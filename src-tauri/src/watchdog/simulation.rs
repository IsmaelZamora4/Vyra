//! Debug-only process fixtures. This module never constructs NativeMagnification.
use super::{client::Client, protocol::Event, session};
use crate::{
    color_engine::{
        magnification::{MagnificationApi, Matrix},
        matrix_for,
    },
    models::Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Display {
    pub matrix: [[f32; 5]; 5],
    pub writes: u32,
    pub fail_write: bool,
    pub fail_read: bool,
    pub bad_readback: bool,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            matrix: matrix_for(&Settings::default()),
            writes: 0,
            fail_write: false,
            fail_read: false,
            bad_readback: false,
        }
    }
}
pub fn safe_root(root: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let temporary = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if root == temporary || !root.starts_with(&temporary) {
        return Err("Simulación limitada a un subdirectorio temporal aislado.".into());
    }
    Ok(root)
}
pub fn read(root: &Path) -> Result<Display, String> {
    let bytes = fs::read(safe_root(root)?.join("mock-display.json")).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn write(root: &Path, display: &Display) -> Result<(), String> {
    crate::durable::json(&safe_root(root)?.join("mock-display.json"), display)
}
pub fn prepare(root: &Path) -> Result<String, String> {
    let root = safe_root(root)?;
    let _lease = super::lease(&root, true)?;
    Ok(session::prepare(&root, &mut FileApi::new(root.clone()))?.session)
}

pub fn missing_helper(root: &Path) -> Result<(), String> {
    let root = safe_root(root)?;
    Client::spawn(
        &root.join("helper-does-not-exist.exe"),
        &root,
        &session::load(&root)?,
        true,
        false,
    )
    .map(|_| ())
}

pub(crate) struct FileApi {
    root: PathBuf,
}
impl FileApi {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
impl MagnificationApi for FileApi {
    fn initialize(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn read(&mut self) -> Result<Matrix, String> {
        let display = read(&self.root)?;
        if display.fail_read {
            Err("Fake read FALSE".into())
        } else {
            Ok(display.matrix)
        }
    }
    fn write(&mut self, matrix: Matrix) -> Result<(), String> {
        let mut display = read(&self.root)?;
        if display.fail_write {
            return Err("Fake write FALSE".into());
        }
        display.writes += 1;
        if !display.bad_readback {
            display.matrix = matrix;
        }
        write(&self.root, &display)
    }
    fn uninitialize(&mut self) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) fn main_fixture(root: &Path) -> Result<i32, String> {
    let root = safe_root(root)?;
    prepare(&root)?;
    let mut client = Client::spawn(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &root,
        &session::load(&root)?,
        true,
        false,
    )?;
    client.arm_after_confirmation()?;
    client.request_apply().map_err(|error| error.to_string())?;
    let started = Instant::now();
    let mut notified = false;
    while started.elapsed() < Duration::from_secs(12) {
        for event in client.poll() {
            if matches!(event, Event::Applied { .. }) && !notified {
                notified = true;
                println!(
                    "{}",
                    serde_json::json!({"helperPid": client.child.id(), "session": client.session})
                );
                std::io::stdout().flush().map_err(|e| e.to_string())?;
            }
        }
        if client.finished {
            return Ok(0);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("Fixture padre agotó su plazo.".into())
}

pub(crate) fn armed_main_fixture(root: &Path) -> Result<i32, String> {
    let root = safe_root(root)?;
    prepare(&root)?;
    let mut client = Client::spawn(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &root,
        &session::load(&root)?,
        true,
        false,
    )?;
    client.arm_after_confirmation()?;
    println!(
        "{}",
        serde_json::json!({"helperPid": client.child.id(), "session": client.session})
    );
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(12) {
        client.poll();
        if client.finished {
            return Ok(0);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("Fixture armado agotó su plazo.".into())
}
