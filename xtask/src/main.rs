use std::env;
use std::error::Error;
use std::process::{Command, ExitCode};

use serde::Deserialize;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match env::args().nth(1).as_deref() {
        None | Some("test") => run_workspace_tests(),
        Some(other) => Err(format!("unsupported xtask command: {other}").into()),
    }
}

fn run_workspace_tests() -> Result<(), Box<dyn Error>> {
    let metadata = load_metadata()?;
    let packages = workspace_packages(&metadata);
    let mut package_results = Vec::with_capacity(packages.len());
    let mut failed_sections = Vec::new();

    println!("lazy-allrounder workspace tests");
    println!("{}", "=".repeat(31));

    for package in packages {
        println!();
        println!("{}", package.name);
        println!("{}", "-".repeat(package.name.len()));

        let mut section_results = Vec::new();
        let sections = test_sections(package);
        let label_width = sections
            .iter()
            .map(|section| section.label.len())
            .max()
            .unwrap_or(3);

        for section in sections {
            let result = run_section(package.name.as_str(), &section)?;
            print_section_result(&result, label_width);
            if !result.success {
                failed_sections.push(format!("{} :: {}", package.name, result.label));
                print_failure_output(&result.output);
            }
            section_results.push(result);
        }

        package_results.push(PackageResult {
            name: package.name.clone(),
            sections: section_results,
        });
    }

    println!();
    println!("workspace summary");
    println!("{}", "-".repeat(17));

    let package_width = package_results
        .iter()
        .map(|package| package.name.len())
        .max()
        .unwrap_or(7);
    let mut total_sections = 0_u32;
    let mut total_tests = 0_u32;
    for package in &package_results {
        let stats = package.stats();
        let status = if package.success() { "PASS" } else { "FAIL" };
        total_sections += package.sections.len() as u32;
        total_tests += stats.executed();
        println!(
            "  [{status}] {:<package_width$} {:>3} tests across {:>2} sections",
            package.name,
            stats.executed(),
            package.sections.len(),
            package_width = package_width
        );
    }

    println!();
    let package_count = package_results.len();
    println!(
        "total: {total_tests} tests across {total_sections} sections in {package_count} packages"
    );

    if failed_sections.is_empty() {
        println!("result: ok");
        return Ok(());
    }

    println!("result: FAILED");
    println!();
    println!("failing sections");
    println!("{}", "-".repeat(16));
    for section in failed_sections {
        println!("  - {section}");
    }

    Err("workspace tests failed".into())
}

fn print_section_result(result: &SectionResult, label_width: usize) {
    let status = if result.success { "PASS" } else { "FAIL" };
    println!(
        "  [{status}] {:<label_width$} {}",
        result.label,
        result.stats.describe(),
        label_width = label_width
    );
}

fn print_failure_output(output: &str) {
    println!("    output:");
    for line in output.lines() {
        println!("      {line}");
    }
}

fn load_metadata() -> Result<Metadata, Box<dyn Error>> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed: {stderr}").into());
    }

    Ok(serde_json::from_slice(&output.stdout)?)
}

fn workspace_packages(metadata: &Metadata) -> Vec<&Package> {
    let mut packages: Vec<&Package> = metadata
        .workspace_members
        .iter()
        .filter_map(|member_id| {
            metadata
                .packages
                .iter()
                .find(|package| &package.id == member_id)
        })
        .collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages
}

fn test_sections(package: &Package) -> Vec<TestSection> {
    let mut sections = Vec::new();

    for target in &package.targets {
        if !target.test {
            continue;
        }

        if target.kind.iter().any(|kind| kind == "lib") {
            sections.push(TestSection::new("lib", vec!["--lib".into()]));
            if target.doctest {
                sections.push(TestSection::new("doc", vec!["--doc".into()]));
            }
        }

        if target.kind.iter().any(|kind| kind == "bin") {
            sections.push(TestSection::new(
                format!("bin:{}", target.name),
                vec!["--bin".into(), target.name.clone()],
            ));
        }

        if target.kind.iter().any(|kind| kind == "test") {
            sections.push(TestSection::new(
                format!("integration:{}", target.name),
                vec!["--test".into(), target.name.clone()],
            ));
        }

        if target.kind.iter().any(|kind| kind == "example") {
            sections.push(TestSection::new(
                format!("example:{}", target.name),
                vec!["--example".into(), target.name.clone()],
            ));
        }
    }

    sections
}

fn run_section(package_name: &str, section: &TestSection) -> Result<SectionResult, Box<dyn Error>> {
    let mut command = Command::new("cargo");
    command.args(["test", "--offline", "--quiet", "-p", package_name]);
    command.args(&section.selector_args);
    command.arg("--");
    command.arg("--color=never");

    let output = command.output()?;
    let rendered_output = join_output(&output.stdout, &output.stderr);
    let stats = TestStats::parse(&rendered_output);

    Ok(SectionResult {
        label: section.label.clone(),
        success: output.status.success(),
        stats,
        output: rendered_output,
    })
}

fn join_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);

    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TestStats {
    passed: u32,
    failed: u32,
    ignored: u32,
}

impl TestStats {
    fn parse(output: &str) -> Self {
        output
            .lines()
            .filter(|line| line.contains("test result:"))
            .fold(Self::default(), |mut totals, line| {
                totals.passed += extract_count(line, " passed;");
                totals.failed += extract_count(line, " failed;");
                totals.ignored += extract_count(line, " ignored;");
                totals
            })
    }

    fn executed(self) -> u32 {
        self.passed + self.failed + self.ignored
    }

    fn describe(self) -> String {
        match (self.executed(), self.failed, self.ignored) {
            (0, _, _) => "0 tests".into(),
            (count, 0, 0) => format!("{count} passed"),
            (count, 0, ignored) => {
                format!("{count} total, {} passed, {ignored} ignored", self.passed)
            }
            (count, failed, 0) => format!("{count} total, {} passed, {failed} failed", self.passed),
            (count, failed, ignored) => format!(
                "{count} total, {} passed, {failed} failed, {ignored} ignored",
                self.passed
            ),
        }
    }
}

fn extract_count(line: &str, suffix: &str) -> u32 {
    line.split(suffix)
        .next()
        .and_then(|prefix| prefix.rsplit_once(' ').map(|(_, count)| count))
        .and_then(|count| count.parse::<u32>().ok())
        .unwrap_or_default()
}

#[derive(Debug)]
struct TestSection {
    label: String,
    selector_args: Vec<String>,
}

impl TestSection {
    fn new(label: impl Into<String>, selector_args: Vec<String>) -> Self {
        Self {
            label: label.into(),
            selector_args,
        }
    }
}

#[derive(Debug)]
struct SectionResult {
    label: String,
    success: bool,
    stats: TestStats,
    output: String,
}

#[derive(Debug)]
struct PackageResult {
    name: String,
    sections: Vec<SectionResult>,
}

impl PackageResult {
    fn success(&self) -> bool {
        self.sections.iter().all(|section| section.success)
    }

    fn stats(&self) -> TestStats {
        self.sections
            .iter()
            .fold(TestStats::default(), |mut totals, section| {
                totals.passed += section.stats.passed;
                totals.failed += section.stats.failed;
                totals.ignored += section.stats.ignored;
                totals
            })
    }
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    doctest: bool,
    test: bool,
}

#[cfg(test)]
mod tests {
    use super::{Package, Target, TestStats, extract_count, test_sections};

    #[test]
    fn parses_multiple_test_result_lines() {
        let output = "\
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: FAILED. 1 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s";

        let stats = TestStats::parse(output);

        assert_eq!(stats.passed, 3);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.ignored, 2);
        assert_eq!(stats.executed(), 6);
    }

    #[test]
    fn extracts_counts_from_summary_line() {
        let line = "test result: ok. 8 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s";

        assert_eq!(extract_count(line, " passed;"), 8);
        assert_eq!(extract_count(line, " failed;"), 1);
        assert_eq!(extract_count(line, " ignored;"), 2);
    }

    #[test]
    fn builds_sections_for_supported_target_kinds() {
        let package = Package {
            id: "pkg".into(),
            name: "lazy-allrounder-demo".into(),
            targets: vec![
                Target {
                    name: "lazy_allrounder_demo".into(),
                    kind: vec!["lib".into()],
                    doctest: true,
                    test: true,
                },
                Target {
                    name: "lazy-allrounder-demo".into(),
                    kind: vec!["bin".into()],
                    doctest: false,
                    test: true,
                },
                Target {
                    name: "smoke".into(),
                    kind: vec!["test".into()],
                    doctest: false,
                    test: true,
                },
            ],
        };

        let sections = test_sections(&package);
        let labels: Vec<_> = sections.into_iter().map(|section| section.label).collect();

        assert_eq!(
            labels,
            vec![
                "lib",
                "doc",
                "bin:lazy-allrounder-demo",
                "integration:smoke"
            ]
        );
    }
}
