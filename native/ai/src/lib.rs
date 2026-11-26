pub mod piper;
pub fn init(dylib: &str) -> Result<(), String> {
    ort::init_from(dylib).commit().map_err(|e| e.to_string())?;
    Ok(())
}
