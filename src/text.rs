//! Plain-text output for the CLI.

use std::fmt::Write;

use omalogi::{
    device::{Info, OnboardState, ProfileSlot},
    editing::{DirectoryRepair, EditPlan, RestorePlan, RestoreReport, TakesEffect, WriteReport},
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

pub fn edit_plan(plan: &EditPlan) -> String {
    let mut out = format!(
        "Profile {} would change (dry run, nothing written):\n",
        plan.profile
    );
    changes(&mut out, &plan.before, &plan.after);
    out
}

pub fn write_report(report: &WriteReport) -> String {
    let mut out = format!("Profile {} updated and verified:\n", report.plan.profile);
    changes(&mut out, &report.plan.before, &report.plan.after);
    takes_effect(&mut out, &report.takes_effect);
    let _ = writeln!(
        out,
        "Backup of the previous memory: {}",
        report.backup.display()
    );
    out
}

fn takes_effect(out: &mut String, takes_effect: &TakesEffect) {
    let _ = match takes_effect {
        TakesEffect::Now => writeln!(out, "The mouse loaded the change and is using it now."),
        TakesEffect::WhenActivated => writeln!(
            out,
            "Not in use yet: the mouse loads a profile when it switches to it, so activate the profile to use the change."
        ),
        TakesEffect::NotLoaded { reason } => {
            writeln!(out, "Saved, but the mouse has not loaded it: {reason}.")
        }
    };
}

pub fn restore_plan(plan: &RestorePlan) -> String {
    if plan.sectors.is_empty() {
        "The mouse already matches the backup.\n".to_owned()
    } else {
        format!(
            "Restoring would write sectors {} (dry run, nothing written).\n",
            sector_list(&plan.sectors)
        )
    }
}

pub fn restore_report(report: &RestoreReport) -> String {
    let mut out = format!(
        "Restored and verified sectors {}.\n",
        sector_list(&report.sectors)
    );
    takes_effect(&mut out, &report.takes_effect);
    let _ = writeln!(
        out,
        "Backup of the memory before restoring: {}",
        report.backup.display()
    );
    out
}

pub fn directory_repair(repair: &DirectoryRepair, dry_run: bool) -> String {
    let mut out = if dry_run {
        "The profile directory's checksum does not match, but its entries check out:\n"
    } else {
        "Repaired and verified the profile directory. It lists:\n"
    }
    .to_owned();
    for entry in &repair.profiles {
        let _ = writeln!(
            out,
            "  Profile {}  sector {:04x}  {}",
            entry.profile,
            entry.sector,
            if entry.enabled { "on" } else { "off" }
        );
    }
    if dry_run {
        let _ = writeln!(
            out,
            "Repairing rewrites only sector {:04x}, with these entries and a new checksum \
             (dry run, nothing written).",
            repair.sector
        );
    }
    out
}

/// A warning for sectors a backup saved with an invalid checksum, or nothing.
pub fn invalid_checksums(sectors: &[String]) -> String {
    if sectors.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "Warning: sector {} failed its checksum and was saved as read; a restore never \
         writes it back.\n",
        sectors.join(", ")
    );
    if sectors.iter().any(|sector| sector == "0000") {
        out.push_str("Run `omalogi profiles repair --dry-run` to check the profile directory.\n");
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

    let _ = writeln!(out, "  Report rate  {}", report_rate(p));
    let _ = writeln!(out, "  DPI stages   {}", dpi_stages(p));

    bindings(out, "Buttons", &p.buttons);
    if p.gshift_buttons.iter().any(|b| *b != Binding::Disabled) {
        bindings(out, "G-Shift", &p.gshift_buttons);
    }
}

fn changes(out: &mut String, before: &Profile, after: &Profile) {
    if before.name != after.name {
        let name = |p: &Profile| p.name.clone().unwrap_or_else(|| "(no name)".to_owned());
        let _ = writeln!(out, "  Name         {} → {}", name(before), name(after));
    }
    if before.report_rate_ms != after.report_rate_ms {
        let _ = writeln!(
            out,
            "  Report rate  {} → {}",
            report_rate(before),
            report_rate(after)
        );
    }
    let stages = |p: &Profile| (p.dpi_stages, p.default_dpi_index, p.shift_dpi_index);
    if stages(before) != stages(after) {
        let _ = writeln!(
            out,
            "  DPI stages   {}  →  {}",
            dpi_stages(before),
            dpi_stages(after)
        );
    }
    binding_changes(out, "Button", &before.buttons, &after.buttons);
    binding_changes(
        out,
        "G-Shift",
        &before.gshift_buttons,
        &after.gshift_buttons,
    );
}

fn binding_changes(out: &mut String, title: &str, before: &[Binding], after: &[Binding]) {
    for (slot, (old, new)) in before.iter().zip(after).enumerate() {
        if old != new {
            let _ = writeln!(
                out,
                "  {title:<7} slot {slot:>2}  {} → {}",
                label::binding(old),
                label::binding(new)
            );
        }
    }
}

fn report_rate(p: &Profile) -> String {
    if p.report_rate_ms == 0 {
        "unknown".to_owned()
    } else {
        format!("{} Hz", 1000 / u16::from(p.report_rate_ms))
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

fn sector_list(sectors: &[u16]) -> String {
    sectors
        .iter()
        .map(|sector| format!("{sector:04x}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn mode(mode: Mode) -> String {
    match mode {
        Mode::Onboard => "onboard (profiles stored on the mouse)".to_owned(),
        Mode::Host => "host (settings applied by software)".to_owned(),
        Mode::Unknown(value) => format!("unknown ({value})"),
    }
}
