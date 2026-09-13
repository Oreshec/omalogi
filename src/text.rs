//! Plain-text output for the CLI.

use std::fmt::Write;

use omalogi::{
    device::{Info, OnboardState, ProfileSlot},
    onboard::{
        Mode,
        format::{Binding, Profile},
        label,
    },
};

// Writing to a String cannot fail, so `writeln!` results are ignored below.

pub fn info(info: &Info) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}  {:04x}:{:04x}  {}",
        info.name, info.vendor_id, info.product_id, info.path
    );
    let firmware: Vec<String> = info
        .firmware
        .iter()
        .map(|fw| {
            if fw.active {
                format!("{} (active)", fw.version)
            } else {
                fw.version.clone()
            }
        })
        .collect();
    let _ = writeln!(out, "Firmware     {}", firmware.join(", "));
    match (info.dpi_values.first(), info.dpi_values.last()) {
        (Some(min), Some(max)) => {
            let _ = writeln!(out, "DPI          {} (sensor range {min}–{max})", info.dpi);
        }
        _ => {
            let _ = writeln!(out, "DPI          {}", info.dpi);
        }
    }
    let rates: Vec<String> = info.report_rates_hz.iter().map(u16::to_string).collect();
    let current = info
        .report_rate_hz
        .map_or_else(|| "unknown".to_owned(), |hz| format!("{hz} Hz"));
    let _ = writeln!(
        out,
        "Report rate  {current} (supports {} Hz)",
        rates.join(", ")
    );
    let _ = writeln!(out, "Mode         {}", mode(info.onboard_mode));
    out
}

pub fn profiles(state: &OnboardState) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Mode: {}. {} of {} profile slots in use.",
        mode(state.mode),
        state.profiles.iter().filter(|slot| slot.enabled).count(),
        state.profiles.len()
    );
    for slot in &state.profiles {
        out.push('\n');
        profile(&mut out, slot);
    }
    out
}

fn profile(out: &mut String, slot: &ProfileSlot) {
    let p = &slot.profile;
    let mut flags = vec![if slot.enabled { "enabled" } else { "disabled" }];
    if slot.active {
        flags.push("active");
    }
    if !slot.crc_valid {
        flags.push("CHECKSUM INVALID");
    }
    let name = p
        .name
        .as_deref()
        .map_or_else(String::new, |name| format!(" \"{name}\""));
    let _ = writeln!(
        out,
        "Profile {}{name}  (sector {:04x}, {})",
        slot.position + 1,
        slot.sector,
        flags.join(", ")
    );
    if !slot.enabled {
        return;
    }

    let rate = if p.report_rate_ms == 0 {
        "unknown".to_owned()
    } else {
        format!("{} Hz", 1000 / u16::from(p.report_rate_ms))
    };
    let _ = writeln!(out, "  Report rate  {rate}");
    let _ = writeln!(out, "  DPI stages   {}", dpi_stages(p));

    bindings(out, "Buttons", &p.buttons);
    if p.gshift_buttons.iter().any(|b| *b != Binding::Disabled) {
        bindings(out, "G-Shift", &p.gshift_buttons);
    }
}

fn dpi_stages(p: &Profile) -> String {
    let stages: Vec<String> = p
        .dpi_stages
        .iter()
        .enumerate()
        .filter_map(|(index, dpi)| {
            let dpi = (*dpi)?;
            Some(if index == usize::from(p.default_dpi_index) {
                format!("[{dpi}]")
            } else {
                dpi.to_string()
            })
        })
        .collect();
    let shift = p
        .dpi_stages
        .get(usize::from(p.shift_dpi_index))
        .copied()
        .flatten()
        .map_or_else(String::new, |dpi| format!("   shift {dpi}"));
    format!("{}{shift}", stages.join("  "))
}

fn bindings(out: &mut String, title: &str, bindings: &[Binding]) {
    let _ = writeln!(out, "  {title}");
    for (slot, binding) in bindings.iter().enumerate() {
        if *binding != Binding::Disabled {
            let _ = writeln!(out, "    slot {slot:>2}  {}", label::binding(binding));
        }
    }
}

fn mode(mode: Mode) -> String {
    match mode {
        Mode::Onboard => "onboard (profiles stored on the mouse)".to_owned(),
        Mode::Host => "host (settings applied by software)".to_owned(),
        Mode::Unknown(value) => format!("unknown ({value})"),
    }
}
