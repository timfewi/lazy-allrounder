use lazy_allrounder_core::error::PortError;

pub fn insert_text_into_focused_app(text: &str) -> Result<(), PortError> {
    platform::insert_text_into_focused_app(text)
}

#[cfg(target_os = "linux")]
#[path = "insertion.rs"]
mod platform;

#[cfg(not(target_os = "linux"))]
mod platform {
    use lazy_allrounder_core::error::PortError;

    pub fn insert_text_into_focused_app(_text: &str) -> Result<(), PortError> {
        Err(PortError::unsupported("focused app insertion"))
    }
}
