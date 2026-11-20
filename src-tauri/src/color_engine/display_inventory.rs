use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayAdapter {
    pub vendor: String,
    pub name: String,
    pub device_name: String,
    pub active: bool,
    pub monitors: Vec<MonitorInfo>,
    #[serde(skip)]
    device_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub name: String,
    pub device_name: String,
    pub active: bool,
}

pub fn enumerate_display_adapters() -> Vec<DisplayAdapter> {
    #[cfg(target_os = "windows")]
    {
        enumerate_windows_display_adapters()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

fn group_adapters(adapters: Vec<DisplayAdapter>) -> Vec<DisplayAdapter> {
    let mut grouped: Vec<DisplayAdapter> = Vec::new();
    for adapter in adapters {
        let key = adapter.device_id.to_ascii_uppercase();
        if let Some(existing) = grouped
            .iter_mut()
            .find(|existing| !key.is_empty() && existing.device_id.eq_ignore_ascii_case(&key))
        {
            existing.active |= adapter.active;
            existing.monitors.extend(adapter.monitors);
        } else {
            grouped.push(adapter);
        }
    }
    grouped
}

fn vendor_from_device_id(device_id: &str) -> &'static str {
    let id = device_id.to_ascii_uppercase();
    if id.contains("VEN_10DE") {
        "NVIDIA"
    } else if id.contains("VEN_1002") {
        "AMD"
    } else if id.contains("VEN_8086") {
        "Intel"
    } else {
        "Otro o desconocido"
    }
}

#[cfg(target_os = "windows")]
fn enumerate_windows_display_adapters() -> Vec<DisplayAdapter> {
    use std::mem::size_of;
    use windows::{
        Win32::Graphics::Gdi::{
            DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICEW, EnumDisplayDevicesW,
        },
        core::PCWSTR,
    };

    fn text(value: &[u16]) -> String {
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..length])
    }

    let mut adapters = Vec::new();
    for index in 0..32 {
        let mut adapter = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        // EnumDisplayDevicesW reports display adapters first, then their child monitors.
        let found =
            unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut adapter, 0).as_bool() };
        if !found {
            break;
        }

        let mut monitors = Vec::new();
        for monitor_index in 0..32 {
            let mut monitor = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            let found = unsafe {
                EnumDisplayDevicesW(
                    PCWSTR::from_raw(adapter.DeviceName.as_ptr()),
                    monitor_index,
                    &mut monitor,
                    1,
                )
                .as_bool()
            };
            if !found {
                break;
            }
            monitors.push(MonitorInfo {
                name: text(&monitor.DeviceString),
                device_name: text(&monitor.DeviceName),
                active: monitor
                    .StateFlags
                    .contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP),
            });
        }

        let device_id = text(&adapter.DeviceID);
        adapters.push(DisplayAdapter {
            vendor: vendor_from_device_id(&device_id).to_string(),
            name: text(&adapter.DeviceString),
            device_name: text(&adapter.DeviceName),
            active: adapter
                .StateFlags
                .contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP),
            monitors,
            device_id,
        });
    }
    group_adapters(adapters)
}

#[cfg(test)]
mod tests {
    use super::{DisplayAdapter, MonitorInfo, group_adapters, vendor_from_device_id};

    fn adapter(device_id: &str, device_name: &str, active: bool, monitor: &str) -> DisplayAdapter {
        DisplayAdapter {
            vendor: "NVIDIA".to_string(),
            name: "GPU".to_string(),
            device_name: device_name.to_string(),
            active,
            monitors: vec![MonitorInfo {
                name: monitor.to_string(),
                device_name: monitor.to_string(),
                active,
            }],
            device_id: device_id.to_string(),
        }
    }

    #[test]
    fn groups_multiple_display_sources_for_same_adapter() {
        let result = group_adapters(vec![
            adapter("PCI\\VEN_10DE&DEV_1", "DISPLAY1", false, "DISPLAY1"),
            adapter("pci\\ven_10de&dev_1", "DISPLAY2", true, "DISPLAY2"),
        ]);
        assert_eq!(result.len(), 1);
        assert!(result[0].active);
        assert_eq!(result[0].monitors.len(), 2);
    }

    #[test]
    fn preserves_distinct_adapters_and_unidentified_entries() {
        let result = group_adapters(vec![
            adapter("PCI\\VEN_10DE&DEV_1", "DISPLAY1", true, "DISPLAY1"),
            adapter("PCI\\VEN_1002&DEV_2", "DISPLAY2", true, "DISPLAY2"),
            adapter("", "DISPLAY3", true, "DISPLAY3"),
            adapter("", "DISPLAY4", true, "DISPLAY4"),
        ]);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn identifies_gpu_vendors_from_windows_hardware_ids() {
        assert_eq!(vendor_from_device_id("PCI\\VEN_10DE&DEV_2684"), "NVIDIA");
        assert_eq!(vendor_from_device_id("PCI\\VEN_1002&DEV_744C"), "AMD");
        assert_eq!(vendor_from_device_id("PCI\\VEN_8086&DEV_56A0"), "Intel");
        assert_eq!(vendor_from_device_id("ROOT\\DISPLAY"), "Otro o desconocido");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn enumerates_display_adapters_without_changing_color_state() {
        let adapters = super::enumerate_display_adapters();
        assert!(adapters.len() <= 32);
        assert!(
            adapters
                .iter()
                .all(|adapter| !adapter.device_name.is_empty())
        );
        assert!(adapters.iter().all(|adapter| adapter.monitors.len() <= 32));
    }
}
