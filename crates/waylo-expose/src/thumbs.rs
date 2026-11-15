//! Progressive thumbnails: each window is captured in its own thread with `grim -T` and the card
//! shows it as soon as it arrives. Nothing is saved to disk.

use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use waylo_core::hypr::Client;
use waylo_render::Image;
use waylo_shell::Waker;

/// Thumbnails shared between the capture threads and the painter; one slot per window.
pub type Shared = Arc<Mutex<Vec<Option<(Image, Instant)>>>>;

pub fn shared(n: usize) -> Shared {
    Arc::new(Mutex::new((0..n).map(|_| None).collect()))
}

/// Launches one capture per window, starting with `first`. Each notifies through `waker` when done.
/// `fit` gives the physical box (width, height) where each thumbnail will be shown: it is captured
/// already at that size so it does not have to be rescaled when painting.
pub fn start(clients: &[Client], first: usize, fit: (f32, f32), shared: &Shared, waker: &Waker) {
    let n = clients.len();
    for k in 0..n {
        let i = (first + k) % n;
        let c = &clients[i];
        if c.stable_id.is_empty() {
            continue;
        }
        let (id, win) = (c.stable_id.clone(), (c.size[0].max(1) as f32, c.size[1].max(1) as f32));
        let (shared, waker) = (shared.clone(), waker.clone());
        std::thread::spawn(move || {
            let scale = (fit.0 / win.0).min(fit.1 / win.1).min(1.0);
            if let Some(img) = capture(&id, scale) {
                if let Ok(mut v) = shared.lock() {
                    v[i] = Some((img, Instant::now()));
                }
                waker.wake();
            }
        });
    }
}

fn capture(stable_id: &str, scale: f32) -> Option<Image> {
    let out = Command::new("grim")
        .args(["-T", stable_id, "-s", &format!("{scale:.3}"), "-t", "ppm", "-"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_ppm(&out.stdout)
}

/// Reads a binary PPM (P6, 8 bits).
fn parse_ppm(data: &[u8]) -> Option<Image> {
    let mut pos = 0;
    let mut token = || -> Option<&str> {
        while data.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !data.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        std::str::from_utf8(&data[start..pos]).ok()
    };
    if token()? != "P6" {
        return None;
    }
    let w: u32 = token()?.parse().ok()?;
    let h: u32 = token()?.parse().ok()?;
    let _max = token()?;
    let start = pos + 1;
    Image::from_rgb(w, h, data.get(start..start + (w * h * 3) as usize)?)
}
