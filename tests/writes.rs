//! Profile writes, backups and restore against an emulated G502 X.

mod support;

use std::{
    fmt::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use omalogi::{
    device::{CLI_SOFTWARE_ID, Session},
    editing::{BackupFile, EditError, ProfileChanges, TakesEffect, save_backup},
    hidraw::SUPPORTED_DEVICES,
    onboard::format::{Binding, crc_ccitt, sector_crc_valid},
};
use support::{FakeG502x, State, fixture_sectors};

const ONBOARD_PROFILES: u16 = 0x8100;
const WRITE_FUNCTIONS: [u8; 3] = [6, 7, 8];

/// A profile number, the changes to try, and a check for the expected refusal.
type RefusalCase = (usize, ProfileChanges, fn(&EditError) -> bool);

struct Harness {
    session: Session,
    state: Arc<Mutex<State>>,
    onboard_index: u8,
    dir: PathBuf,
}

impl Harness {
    async fn new(name: &str) -> Self {
        let device = FakeG502x::new();
        let state = device.state();
        let onboard_index = device.feature_index(ONBOARD_PROFILES);
        let session = Session::connect(
            device,
            SUPPORTED_DEVICES[0],
            "emulated".to_owned(),
            CLI_SOFTWARE_ID,
        )
        .await
        .expect("session starts");
        let dir =
            std::env::temp_dir().join(format!("omalogi-writes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self {
            session,
            state,
            onboard_index,
            dir,
        }
    }

    fn backup_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    fn write_requests(&self) -> usize {
        self.state
            .lock()
            .expect("state")
            .requests
            .iter()
            .filter(|r| r[2] == self.onboard_index && WRITE_FUNCTIONS.contains(&(r[3] >> 4)))
            .count()
    }

    fn sector(&self, sector: u16) -> Vec<u8> {
        self.state.lock().expect("state").sectors[&sector].clone()
    }

    fn committed(&self) -> Vec<u16> {
        self.state.lock().expect("state").committed.clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
        out
    })
}

fn rate(hz: u16) -> ProfileChanges {
    ProfileChanges {
        report_rate_hz: Some(hz),
        ..ProfileChanges::default()
    }
}

#[tokio::test]
async fn dry_run_plans_without_writing() {
    let mut h = Harness::new("plan").await;
    let changes = ProfileChanges {
        dpi_stages: Some(vec![400, 800, 1600]),
        default_dpi: Some(800),
        report_rate_hz: Some(500),
        ..ProfileChanges::default()
    };

    let plan = h
        .session
        .plan_profile_changes(1, &changes)
        .await
        .expect("plan");

    assert_eq!(
        plan.after.dpi_stages,
        [Some(400), Some(800), Some(1600), None, None]
    );
    // Default follows --default-dpi; shift keeps its 800 DPI value at its new position.
    assert_eq!(
        (plan.after.default_dpi_index, plan.after.shift_dpi_index),
        (1, 1)
    );
    assert_eq!(plan.after.report_rate_ms, 2);
    assert_eq!(h.write_requests(), 0);
    assert_eq!(h.sector(1), fixture_sectors()[&1]);
}

#[tokio::test]
async fn writes_are_backed_up_and_verified() {
    let mut h = Harness::new("apply").await;
    let original = fixture_sectors();
    let ctrl_c = Binding::Key {
        modifiers: 0x01,
        key: 0x06,
    };
    let changes = ProfileChanges {
        report_rate_hz: Some(500),
        buttons: vec![(6, ctrl_c)],
        ..ProfileChanges::default()
    };
    let backup = h.backup_path("before-edit");

    let report = h
        .session
        .apply_profile_changes(1, &changes, &backup)
        .await
        .expect("write succeeds");

    assert_eq!(report.plan.after.buttons[6], ctrl_c);
    let written = h.sector(1);
    assert!(sector_crc_valid(&written));
    assert_ne!(written, original[&1]);
    assert_eq!(h.committed(), [1]);

    let saved = BackupFile::load(&backup).expect("backup file loads");
    assert_eq!(saved.sectors["0001"], hex(&original[&1]));

    let onboard = h.session.onboard().await.expect("reads back");
    assert_eq!(onboard.profiles[0].profile.report_rate_ms, 2);
    assert_eq!(onboard.profiles[0].profile.buttons[6], ctrl_c);

    // An existing backup is never overwritten, and nothing is written without one.
    let writes_before = h.write_requests();
    assert!(matches!(
        h.session
            .apply_profile_changes(1, &rate(250), &backup)
            .await,
        Err(EditError::SaveBackup { .. })
    ));
    assert_eq!(h.write_requests(), writes_before);
}

#[tokio::test]
async fn invalid_changes_are_refused_without_writing() {
    let mut h = Harness::new("invalid").await;
    let dpi = |stages: Vec<u16>| ProfileChanges {
        dpi_stages: Some(stages),
        ..ProfileChanges::default()
    };
    let button = |slot: usize| ProfileChanges {
        buttons: vec![(slot, Binding::Disabled)],
        ..ProfileChanges::default()
    };

    let cases: Vec<RefusalCase> = vec![
        (1, dpi(vec![125]), |e| {
            matches!(e, EditError::DpiNotSupported(125))
        }),
        (1, dpi(vec![800; 6]), |e| {
            matches!(e, EditError::DpiStageCount(6))
        }),
        (1, rate(300), |e| {
            matches!(e, EditError::ReportRateNotSupported { hz: 300, .. })
        }),
        (
            1,
            ProfileChanges {
                default_dpi: Some(999),
                ..ProfileChanges::default()
            },
            |e| matches!(e, EditError::DpiNotAStage(999)),
        ),
        (1, button(14), |e| {
            matches!(
                e,
                EditError::SlotNotEditable {
                    table: "buttons",
                    slot: 14
                }
            )
        }),
        (1, rate(1000), |e| matches!(e, EditError::NoChanges)),
        (9, rate(500), |e| {
            matches!(
                e,
                EditError::NoSuchProfile {
                    number: 9,
                    count: 5
                }
            )
        }),
    ];
    for (number, changes, expected) in cases {
        let error = h
            .session
            .plan_profile_changes(number, &changes)
            .await
            .expect_err("change is refused");
        assert!(expected(&error), "unexpected error: {error}");

        let backup = h.backup_path("refused");
        assert!(
            h.session
                .apply_profile_changes(number, &changes, &backup)
                .await
                .is_err()
        );
        assert!(!backup.exists(), "no backup for a refused change");
    }
    assert_eq!(h.write_requests(), 0);
}

#[tokio::test]
async fn wheel_slots_are_editable_where_the_device_binds_them() {
    let mut h = Harness::new("wheel").await;
    let scroll = ProfileChanges {
        buttons: vec![(12, Binding::Disabled)],
        ..ProfileChanges::default()
    };
    // Profile 2 binds slot 12 to scroll up; profile 1 leaves it unbound.
    assert!(h.session.plan_profile_changes(2, &scroll).await.is_ok());
    assert!(matches!(
        h.session.plan_profile_changes(1, &scroll).await,
        Err(EditError::SlotNotEditable { slot: 12, .. })
    ));
}

#[tokio::test]
async fn a_corrupted_write_is_rolled_back() {
    let mut h = Harness::new("corrupt").await;
    let original = fixture_sectors();
    h.state.lock().expect("state").corrupt_next_write = true;

    let result = h
        .session
        .apply_profile_changes(1, &rate(500), &h.backup_path("before"))
        .await;

    assert!(
        matches!(
            result,
            Err(EditError::VerifyFailed {
                sector: 1,
                restored: true
            })
        ),
        "{result:?}"
    );
    assert_eq!(h.sector(1), original[&1]);
    assert_eq!(h.committed(), [1, 1]);
}

#[tokio::test]
async fn restores_profile_memory_from_a_backup() {
    let mut h = Harness::new("restore").await;
    let original = fixture_sectors();
    let first = h.backup_path("original");
    h.session
        .apply_profile_changes(1, &rate(500), &first)
        .await
        .expect("first edit");
    h.session
        .apply_profile_changes(2, &rate(250), &h.backup_path("second"))
        .await
        .expect("second edit");

    let backup = BackupFile::load(&first).expect("backup loads");
    let plan = h.session.plan_restore(&backup).await.expect("plan");
    assert_eq!(plan.sectors, [1, 2]);

    h.session
        .restore(&backup, &h.backup_path("before-restore"))
        .await
        .expect("restore succeeds");

    for sector in [0x0000, 0x0001, 0x0002] {
        assert_eq!(h.sector(sector), original[&sector], "sector {sector:04x}");
    }
    assert!(h.committed().iter().all(|&sector| sector < 0x0100));
    assert!(matches!(
        h.session.restore(&backup, &h.backup_path("again")).await,
        Err(EditError::AlreadyRestored)
    ));
}

#[tokio::test]
async fn a_write_to_the_active_profile_is_loaded_right_away() {
    let mut h = Harness::new("active").await;
    let report = h
        .session
        .apply_profile_changes(1, &rate(500), &h.backup_path("before"))
        .await
        .expect("write succeeds");

    assert_eq!(report.takes_effect, TakesEffect::Now);
    let state = h.state.lock().expect("state");
    // Profile 1 was active: the mouse switched to profile 2 and back, loading the new memory.
    assert_eq!(state.loads, [2, 1]);
    assert_eq!(state.current_profile, 1);
    assert_eq!(
        state.loaded_sector.as_deref(),
        Some(state.sectors[&1].as_slice())
    );
}

#[tokio::test]
async fn a_write_to_an_inactive_profile_applies_when_activated() {
    let mut h = Harness::new("inactive").await;
    let report = h
        .session
        .apply_profile_changes(2, &rate(500), &h.backup_path("before"))
        .await
        .expect("write succeeds");

    assert_eq!(report.takes_effect, TakesEffect::WhenActivated);
    let state = h.state.lock().expect("state");
    assert!(state.loads.is_empty(), "the active profile is left alone");
    assert_eq!(state.current_profile, 1);
}

#[tokio::test]
async fn the_only_enabled_profile_is_written_but_reported_as_not_loaded() {
    let mut h = Harness::new("single").await;
    {
        let mut state = h.state.lock().expect("state");
        let directory = state.sectors.get_mut(&0).expect("directory sector");
        // Entries are [sector hi, sector lo, enabled, reserved]; turn profile 2 off.
        directory[6] = 0;
        let body = directory.len() - 2;
        let crc = crc_ccitt(&directory[..body]).to_be_bytes();
        directory[body..].copy_from_slice(&crc);
    }

    let report = h
        .session
        .apply_profile_changes(1, &rate(500), &h.backup_path("before"))
        .await
        .expect("write succeeds");

    assert!(
        matches!(&report.takes_effect, TakesEffect::NotLoaded { reason } if reason.contains("no other profile")),
        "{:?}",
        report.takes_effect
    );
    let state = h.state.lock().expect("state");
    assert!(state.loads.is_empty());
    assert_eq!(state.committed, [1]);
}

#[tokio::test]
async fn a_switch_the_mouse_ignores_is_reported_not_hidden() {
    let mut h = Harness::new("ignored").await;
    h.state.lock().expect("state").ignore_profile_switch = true;

    let report = h
        .session
        .apply_profile_changes(1, &rate(500), &h.backup_path("before"))
        .await
        .expect("the verified write is still reported");

    assert!(
        matches!(&report.takes_effect, TakesEffect::NotLoaded { reason } if reason.contains("profile 2")),
        "{:?}",
        report.takes_effect
    );
    assert_eq!(h.state.lock().expect("state").current_profile, 1);
}

#[tokio::test]
async fn restoring_the_active_profile_loads_it() {
    let mut h = Harness::new("restore-load").await;
    let before = h.backup_path("before");
    h.session
        .apply_profile_changes(1, &rate(500), &before)
        .await
        .expect("edit");
    h.state.lock().expect("state").loads.clear();

    let backup = BackupFile::load(&before).expect("backup loads");
    let report = h
        .session
        .restore(&backup, &h.backup_path("before-restore"))
        .await
        .expect("restore succeeds");

    assert_eq!(report.takes_effect, TakesEffect::Now);
    let state = h.state.lock().expect("state");
    assert_eq!(state.loads, [2, 1]);
    assert_eq!(
        state.loaded_sector.as_deref(),
        Some(fixture_sectors()[&1].as_slice())
    );
}

#[tokio::test]
async fn a_profile_that_reads_back_corrupt_is_never_edited() {
    let mut h = Harness::new("corrupt-read").await;
    h.state
        .lock()
        .expect("state")
        .sectors
        .get_mut(&1)
        .expect("sector 1")[40] ^= 0xFF;

    let result = h
        .session
        .apply_profile_changes(1, &rate(500), &h.backup_path("before"))
        .await;

    assert!(
        matches!(result, Err(EditError::CorruptSector { sector: 1 })),
        "{result:?}"
    );
    assert_eq!(h.write_requests(), 0);
    assert!(!h.backup_path("before").exists());
}

#[tokio::test]
async fn turns_profiles_on_and_off_in_the_directory() {
    let mut h = Harness::new("enable").await;
    let original = fixture_sectors();

    h.session
        .set_profile_enabled(3, true)
        .await
        .expect("profile 3 turns on");
    let state = h.session.onboard().await.expect("state");
    let enabled: Vec<_> = state.profiles.iter().map(|slot| slot.enabled).collect();
    assert_eq!(enabled, [true, true, true, false, false]);
    assert_eq!(h.committed(), [0]);
    assert!(sector_crc_valid(&h.sector(0)));

    h.session
        .set_profile_enabled(3, false)
        .await
        .expect("profile 3 turns off again");
    assert_eq!(
        h.sector(0),
        original[&0],
        "the directory is exactly as before"
    );

    assert!(matches!(
        h.session.set_profile_enabled(3, false).await,
        Err(EditError::NoChanges)
    ));
    assert!(matches!(
        h.session.set_profile_enabled(1, false).await,
        Err(EditError::ActiveProfile(1))
    ));
    assert!(matches!(
        h.session.set_profile_enabled(9, true).await,
        Err(EditError::NoSuchProfile { number: 9, .. })
    ));

    // With profile 2 off and profile 3 selected, profile 1 is the last one on.
    h.session
        .set_profile_enabled(2, false)
        .await
        .expect("profile 2 turns off");
    h.state.lock().expect("state").current_profile = 3;
    assert!(matches!(
        h.session.set_profile_enabled(1, false).await,
        Err(EditError::LastEnabledProfile(1))
    ));
}

#[tokio::test]
async fn names_a_profile_and_clears_the_name() {
    let mut h = Harness::new("name").await;
    let named = |name: &str| ProfileChanges {
        name: Some(name.to_owned()),
        ..ProfileChanges::default()
    };

    let report = h
        .session
        .apply_profile_changes(3, &named("Omalogi Test"), &h.backup_path("named"))
        .await
        .expect("name is written");
    assert_eq!(report.plan.after.name.as_deref(), Some("Omalogi Test"));
    let state = h.session.onboard().await.expect("state");
    assert_eq!(
        state.profiles[2].profile.name.as_deref(),
        Some("Omalogi Test")
    );

    h.session
        .apply_profile_changes(3, &named(""), &h.backup_path("cleared"))
        .await
        .expect("name is cleared");
    assert_eq!(
        h.sector(3),
        fixture_sectors()[&3],
        "back to the unwritten name"
    );

    let writes = h.write_requests();
    for bad in ["Ömalogi", &"x".repeat(48)] {
        assert!(matches!(
            h.session.plan_profile_changes(3, &named(bad)).await,
            Err(EditError::InvalidName(_))
        ));
    }
    assert_eq!(h.write_requests(), writes);
}

#[tokio::test]
async fn refuses_backups_that_do_not_fit() {
    let mut h = Harness::new("mismatch").await;
    let good = h.backup_path("good");
    let backup = h.session.backup().await.expect("backup");
    save_backup(&backup, &good).expect("save");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&good).expect("read")).expect("json");

    let dir = h.dir.clone();
    let tampered = |name: &str, edit: fn(&mut serde_json::Value)| {
        let mut copy = json.clone();
        edit(&mut copy);
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, copy.to_string()).expect("write tampered backup");
        BackupFile::load(&path).expect("tampered backup still parses")
    };

    let other_layout = tampered("layout", |v| v["description"]["profile_format"] = 5.into());
    assert!(matches!(
        h.session.plan_restore(&other_layout).await,
        Err(EditError::BackupMismatch(_))
    ));

    let other_device = tampered("device", |v| v["product_id"] = 0xC08B.into());
    assert!(matches!(
        h.session.plan_restore(&other_device).await,
        Err(EditError::BackupMismatch(_))
    ));

    let bad_crc = tampered("crc", |v| {
        let sector = v["sectors"]["0001"].as_str().expect("hex").to_owned();
        v["sectors"]["0001"] = format!("ff{}", &sector[2..]).into();
    });
    assert!(matches!(
        h.session.plan_restore(&bad_crc).await,
        Err(EditError::InvalidBackup { .. })
    ));

    // Factory sectors in a backup are ignored, never written.
    let rom = tampered("rom", |v| {
        let sector = v["sectors"]["0101"].as_str().expect("hex").to_owned();
        v["sectors"]["0101"] = format!("00{}", &sector[2..]).into();
    });
    assert!(matches!(
        h.session.restore(&rom, &h.backup_path("rom-before")).await,
        Err(EditError::AlreadyRestored)
    ));
    assert_eq!(h.write_requests(), 0);
}

/// Damages the directory as reported on a G502 X: the entries intact, one padding byte
/// changed and the checksum never written.
fn damage_directory(state: &Mutex<State>) {
    let mut state = state.lock().expect("state");
    let directory = state.sectors.get_mut(&0).expect("sector 0");
    directory[22] = 0x01;
    let crc_at = directory.len() - 2;
    directory[crc_at..].copy_from_slice(&[0xFF, 0xFF]);
}

#[tokio::test]
async fn a_damaged_directory_is_repaired_from_its_own_entries() {
    let mut h = Harness::new("repair").await;
    let original = fixture_sectors();
    damage_directory(&h.state);

    let refused = h.session.onboard().await.expect_err("profiles are refused");
    assert!(
        refused.to_string().contains("omalogi profiles repair"),
        "{refused}"
    );
    assert!(matches!(
        h.session
            .apply_profile_changes(3, &rate(500), &h.backup_path("edit"))
            .await,
        Err(EditError::Session(_))
    ));

    // The backup still captures everything, and flags the directory.
    let backup = h.session.backup().await.expect("backup of damaged memory");
    assert_eq!(backup.invalid_checksums, ["0000"]);
    for sector in ["0000", "0001", "0002", "0003", "0004", "0005"] {
        assert!(backup.sectors.contains_key(sector), "sector {sector}");
    }

    let plan = h.session.plan_directory_repair().await.expect("repairable");
    let listed: Vec<_> = plan
        .profiles
        .iter()
        .map(|entry| (entry.profile, entry.sector, entry.enabled))
        .collect();
    assert_eq!(
        listed,
        [
            (1, 1, true),
            (2, 2, true),
            (3, 3, false),
            (4, 4, false),
            (5, 5, false)
        ]
    );
    assert_eq!(h.write_requests(), 0, "planning writes nothing");

    h.session.repair_directory().await.expect("repaired");
    assert_eq!(h.committed(), [0], "only the directory is written");
    assert_eq!(h.sector(0), original[&0], "byte for byte as it was");
    assert_eq!(
        h.session
            .onboard()
            .await
            .expect("profiles read")
            .profiles
            .len(),
        5
    );
    assert!(matches!(
        h.session.plan_directory_repair().await,
        Err(EditError::DirectoryIntact)
    ));
}

#[tokio::test]
async fn a_directory_that_cannot_be_rebuilt_is_left_alone() {
    let mut h = Harness::new("unrepairable").await;
    assert!(matches!(
        h.session.plan_directory_repair().await,
        Err(EditError::DirectoryIntact)
    ));

    // A profile the directory lists is damaged too.
    damage_directory(&h.state);
    h.state
        .lock()
        .expect("state")
        .sectors
        .get_mut(&2)
        .expect("sector 2")[40] ^= 0xFF;
    let result = h.session.repair_directory().await;
    assert!(
        matches!(
            result,
            Err(EditError::DamagedProfile {
                number: 2,
                sector: 2
            })
        ),
        "{result:?}"
    );

    // Entries that do not make sense.
    h.state
        .lock()
        .expect("state")
        .sectors
        .get_mut(&0)
        .expect("sector 0")[5] = 0x01;
    let result = h.session.repair_directory().await;
    assert!(
        matches!(result, Err(EditError::DirectoryUnrepairable(_))),
        "{result:?}"
    );
    assert!(
        result
            .expect_err("refused")
            .to_string()
            .contains("sector 0x0001 is listed twice")
    );
    assert_eq!(h.write_requests(), 0);
}

#[tokio::test]
async fn restore_works_past_a_damaged_directory() {
    let mut h = Harness::new("restore-damaged").await;
    let original = fixture_sectors();
    let good = h.backup_path("good");
    save_backup(&h.session.backup().await.expect("backup"), &good).expect("save");
    damage_directory(&h.state);

    let before = h.backup_path("before-restore");
    let report = h
        .session
        .restore(&BackupFile::load(&good).expect("loads"), &before)
        .await
        .expect("restore succeeds");
    assert_eq!(report.sectors, [0]);
    assert_eq!(
        report.takes_effect,
        TakesEffect::Now,
        "no profile to load for the directory alone"
    );
    assert_eq!(h.sector(0), original[&0]);

    // The damaged state was kept, flagged, and is never written back.
    let damaged = BackupFile::load(&before).expect("loads");
    assert_eq!(damaged.invalid_checksums, ["0000"]);
    let refused = h.session.plan_restore(&damaged).await;
    assert!(
        matches!(&refused, Err(EditError::InvalidBackup { reason, .. })
            if reason.contains("never written back")),
        "{refused:?}"
    );
    assert_eq!(h.committed(), [0]);
}
