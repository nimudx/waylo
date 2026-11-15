use std::path::PathBuf;
use std::time::Duration;
use std::{fs, thread};

use ddc_hi::{Ddc, Display};

/// Something whose brightness can be read and changed, as a percentage (0-100).
pub trait Brightness {
    fn name(&self) -> String;
    fn get(&mut self) -> Result<u8, String>;
    fn set(&mut self, percent: u8) -> Result<(), String>;
}

/// Laptop screen, controlled through sysfs.
struct Laptop {
    dir: PathBuf,
}

impl Laptop {
    fn detect() -> Option<Self> {
        let dir = fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
        Some(Self { dir })
    }

    fn read(&self, file: &str) -> Result<u32, String> {
        fs::read_to_string(self.dir.join(file))
            .map_err(|e| format!("{file}: {e}"))?
            .trim()
            .parse()
            .map_err(|e| format!("{file}: {e}"))
    }
}

impl Brightness for Laptop {
    fn name(&self) -> String {
        "laptop".into()
    }

    fn get(&mut self) -> Result<u8, String> {
        let (now, max) = (self.read("brightness")?, self.read("max_brightness")?);
        Ok((now * 100 / max) as u8)
    }

    fn set(&mut self, percent: u8) -> Result<(), String> {
        let max = self.read("max_brightness")?;
        // Minimum 1 so the screen is never switched off completely.
        let value = (percent as u32 * max / 100).max(1);
        fs::write(self.dir.join("brightness"), value.to_string())
            .map_err(|e| format!("no pude escribir el brillo: {e}"))
    }
}

/// External monitor, controlled over DDC/CI (VCP 0x10 = luminance).
struct External {
    display: Display,
    label: String,
}

const VCP_BRIGHTNESS: u8 = 0x10;

impl External {
    /// Reads (value, maximum) with retries. A reply with maximum 0 is DDC garbage
    /// (it happens right after the screen reloads): it is discarded, because with it `set`
    /// would compute 0 and turn the brightness off.
    fn read(&mut self) -> Result<(u16, u16), String> {
        let mut last = String::from("sin respuesta");
        for attempt in 0..4 {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(150));
            }
            match self.display.handle.get_vcp_feature(VCP_BRIGHTNESS) {
                Ok(v) if v.maximum() > 0 => return Ok((v.value(), v.maximum())),
                Ok(_) => last = "lectura inválida del monitor (máximo 0)".into(),
                Err(e) => last = e.to_string(),
            }
        }
        Err(last)
    }
}

impl Brightness for External {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn get(&mut self) -> Result<u8, String> {
        let (value, max) = self.read()?;
        Ok((value as u32 * 100 / max as u32) as u8)
    }

    fn set(&mut self, percent: u8) -> Result<(), String> {
        let (_, max) = self.read()?;
        self.display
            .handle
            .set_vcp_feature(VCP_BRIGHTNESS, (percent as u32 * max as u32 / 100) as u16)
            .map_err(|e| e.to_string())
    }
}

/// All devices with controllable brightness.
pub fn devices() -> Vec<Box<dyn Brightness>> {
    let mut list: Vec<Box<dyn Brightness>> = Vec::new();
    if let Some(laptop) = Laptop::detect() {
        list.push(Box::new(laptop));
    }
    for mut display in Display::enumerate() {
        // The laptop panel also shows up here but does not answer DDC: it is discarded.
        if !matches!(display.handle.get_vcp_feature(VCP_BRIGHTNESS), Ok(v) if v.maximum() > 0) {
            continue;
        }
        let label = display
            .info
            .model_name
            .clone()
            .unwrap_or_else(|| display.info.id.clone());
        list.push(Box::new(External { display, label }));
    }
    list
}
