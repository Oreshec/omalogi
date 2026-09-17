//! `omalogi serve` against an emulated G502 X, over an in-memory pipe.

mod support;

use std::{
    error::Error,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use omalogi::{device::Session, hidraw::SUPPORTED_DEVICES, serve};
use serde_json::{Value, json};
use support::{FakeG502x, fixture_sectors};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// A directory for one test; tests run in parallel, so none may share one.
fn test_dir(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("omalogi-serve-{}-{test}", std::process::id()))
}

fn backup_in(test: &str) -> Result<PathBuf, Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(test_dir(test).join(format!("backup-{n}.json")))
}

#[tokio::test]
async fn edits_undo_and_activation_over_json_lines() {
    let dir = test_dir("edits");
    let _ = std::fs::remove_dir_all(&dir);
    let device = FakeG502x::new();
    let state = device.state();
    let session = Session::connect(
        device,
        SUPPORTED_DEVICES[0],
        "emulated".to_owned(),
        serve::SOFTWARE_ID,
    )
    .await
    .expect("session starts");
    let server = serve::Server::new(session, Some(dir.join("device.lock")), |_| {
        backup_in("edits")
    });

    let (client, server_side) = tokio::io::duplex(1 << 20);
    let (server_read, server_write) = tokio::io::split(server_side);
    let (client_read, mut client_write) = tokio::io::split(client);

    let script = async {
        let mut responses = BufReader::new(client_read).lines();
        let mut next_id = 0;
        let mut ask = async |request: Value| -> Value {
            next_id += 1;
            let mut request = request;
            request["id"] = next_id.into();
            let line = format!("{request}\n");
            client_write.write_all(line.as_bytes()).await.expect("send");
            let reply = responses.next_line().await.expect("read").expect("a reply");
            let reply: Value = serde_json::from_str(&reply).expect("JSON reply");
            assert_eq!(reply["id"], next_id, "replies answer requests in order");
            reply
        };

        let reply = ask(json!({ "cmd": "state" })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["info"]["name"], "G502 X");
        assert_eq!(
            reply["result"]["onboard"]["profiles"]
                .as_array()
                .map(Vec::len),
            Some(5)
        );

        // Profile 1 is active: the write is loaded at once.
        let reply = ask(json!({ "cmd": "apply", "profile": 1, "rate": 500 })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        let result = &reply["result"];
        assert_eq!(result["takes_effect"]["state"], "now");
        assert_eq!(result["slot"]["profile"]["report_rate_ms"], 2);
        assert_eq!(result["undo"], 1);
        let backup = PathBuf::from(result["backup"].as_str().expect("backup path"));
        assert!(backup.exists());

        let reply =
            ask(json!({ "cmd": "apply", "profile": 1, "buttons": { "6": "key:ctrl+c" } })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(
            reply["result"]["slot"]["actions"]["buttons"][6],
            "key:ctrl+c"
        );
        assert_eq!(reply["result"]["undo"], 2);
        assert_eq!(
            reply["result"]["backup"],
            backup.to_str().expect("utf-8"),
            "one backup per session"
        );

        // The same changes again: nothing to write, nothing to undo.
        let reply =
            ask(json!({ "cmd": "apply", "profile": 1, "buttons": { "6": "key:ctrl+c" } })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["takes_effect"], Value::Null);
        assert_eq!(reply["result"]["undo"], 2);

        // Bad input is refused before anything is written.
        let committed = state.lock().expect("state").committed.len();
        let reply = ask(json!({ "cmd": "apply", "profile": 1, "buttons": { "3": "jump" } })).await;
        assert_eq!(reply["ok"], false);
        assert!(
            reply["error"]
                .as_str()
                .expect("error")
                .contains("unknown action")
        );
        let reply = ask(json!({ "cmd": "fly" })).await;
        assert_eq!(reply["ok"], false);
        assert!(
            reply["error"]
                .as_str()
                .expect("error")
                .starts_with("bad request")
        );
        assert_eq!(state.lock().expect("state").committed.len(), committed);

        // Undo walks back one write at a time, loading the active profile each time.
        let reply = ask(json!({ "cmd": "undo" })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["takes_effect"]["state"], "now");
        assert_eq!(reply["result"]["slot"]["profile"]["report_rate_ms"], 2);
        assert_eq!(reply["result"]["undo"], 1);
        let reply = ask(json!({ "cmd": "undo" })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["undo"], 0);
        {
            let state = state.lock().expect("state");
            assert_eq!(state.sectors[&1], fixture_sectors()[&1]);
            assert_eq!(
                state.loaded_sector.as_deref(),
                Some(fixture_sectors()[&1].as_slice())
            );
        }
        let reply = ask(json!({ "cmd": "undo" })).await;
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"], "there is nothing to undo");

        let reply = ask(json!({ "cmd": "activate", "profile": 2 })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(state.lock().expect("state").current_profile, 2);

        // Closing stdin ends the server.
        drop(client_write);
    };

    let (served, ()) = tokio::join!(
        server.run(BufReader::new(server_read), server_write),
        script
    );
    served.expect("server ends cleanly");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_damaged_directory_is_reported_and_repaired() {
    let dir = test_dir("repair");
    let _ = std::fs::remove_dir_all(&dir);
    let device = FakeG502x::new();
    let state = device.state();
    {
        let mut state = state.lock().expect("state");
        let directory = state.sectors.get_mut(&0).expect("sector 0");
        let crc_at = directory.len() - 2;
        directory[crc_at..].copy_from_slice(&[0xFF, 0xFF]);
    }
    let session = Session::connect(
        device,
        SUPPORTED_DEVICES[0],
        "emulated".to_owned(),
        serve::SOFTWARE_ID,
    )
    .await
    .expect("session starts");
    let server = serve::Server::new(session, Some(dir.join("device.lock")), |_| {
        backup_in("repair")
    });

    let (client, server_side) = tokio::io::duplex(1 << 20);
    let (server_read, server_write) = tokio::io::split(server_side);
    let (client_read, mut client_write) = tokio::io::split(client);

    let script = async {
        let mut responses = BufReader::new(client_read).lines();
        let mut ask = async |request: Value| -> Value {
            let line = format!("{request}\n");
            client_write.write_all(line.as_bytes()).await.expect("send");
            let reply = responses.next_line().await.expect("read").expect("a reply");
            serde_json::from_str(&reply).expect("JSON reply")
        };

        let reply = ask(json!({ "id": 1, "cmd": "state" })).await;
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["kind"], "directory_checksum");

        let reply = ask(json!({ "id": 2, "cmd": "apply", "profile": 3, "rate": 500 })).await;
        assert_eq!(reply["kind"], "directory_checksum", "{reply}");

        let reply = ask(json!({ "id": 3, "cmd": "repair_directory" })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(
            reply["result"]["onboard"]["profiles"]
                .as_array()
                .map(Vec::len),
            Some(5)
        );
        let backup = PathBuf::from(reply["result"]["backup"].as_str().expect("backup path"));
        assert!(backup.exists(), "backed up before repairing");
        assert_eq!(
            state.lock().expect("state").sectors[&0],
            fixture_sectors()[&0]
        );

        let reply = ask(json!({ "id": 4, "cmd": "state" })).await;
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["helper"]["protocol"], serve::PROTOCOL);

        let reply = ask(json!({ "id": 5, "cmd": "repair_directory" })).await;
        assert_eq!(reply["ok"], false);
        assert!(
            reply.get("kind").is_none(),
            "an intact directory needs nothing: {reply}"
        );

        drop(client_write);
    };

    let (served, ()) = tokio::join!(
        server.run(BufReader::new(server_read), server_write),
        script
    );
    served.expect("server ends cleanly");
    let _ = std::fs::remove_dir_all(&dir);
}
