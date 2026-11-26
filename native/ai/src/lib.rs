pub mod piper;
pub mod sep;
pub mod sing;
pub mod vc;
pub mod whisper;
pub fn init(dylib: &str) -> Result<(), String> {
    ort::init_from(dylib).commit().map_err(|e| e.to_string())?;
    Ok(())
}
