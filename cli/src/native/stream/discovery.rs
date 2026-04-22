use serde_json::{json, Value};
use std::path::Path;

use crate::connection::get_socket_dir;

use super::parse_stream_metadata;

pub(super) fn discover_sessions() -> String {
    let dir = get_socket_dir();
    let mut sessions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(session) = name_str.strip_suffix(".stream") {
                if let Ok(stream_str) = std::fs::read_to_string(entry.path()) {
                    if let Some(metadata) = parse_stream_metadata(&stream_str) {
                        let pid_path = dir.join(format!("{}.pid", session));
                        if is_process_alive(&pid_path) {
                            let engine_path = dir.join(format!("{}.engine", session));
                            let engine = std::fs::read_to_string(&engine_path)
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .unwrap_or_else(|| "chrome".to_string());

                            let provider_path = dir.join(format!("{}.provider", session));
                            let provider = std::fs::read_to_string(&provider_path)
                                .ok()
                                .filter(|s| !s.trim().is_empty());

                            let extensions = read_extensions_metadata(&dir, session);

                            let mut entry = json!({
                                "session": session,
                                "addr": metadata.addr,
                                "port": metadata.port,
                                "engine": engine.trim(),
                            });
                            if let Some(ref p) = provider {
                                entry["provider"] = json!(p.trim());
                            }
                            if !extensions.is_empty() {
                                entry["extensions"] = json!(extensions);
                            }
                            sessions.push(entry);
                        } else {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }

    serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string())
}

fn read_extensions_metadata(dir: &std::path::Path, session: &str) -> Vec<Value> {
    let ext_path = dir.join(format!("{}.extensions", session));
    let ext_str = match std::fs::read_to_string(&ext_path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    ext_str
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .filter_map(|path| {
            let manifest_path = std::path::Path::new(path).join("manifest.json");
            let manifest_str = std::fs::read_to_string(&manifest_path).ok()?;
            let manifest: Value = serde_json::from_str(&manifest_str).ok()?;

            let name = manifest
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let version = manifest
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = manifest
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mut ext = json!({
                "name": name,
                "version": version,
                "path": path,
            });
            if let Some(desc) = description {
                ext["description"] = json!(desc);
            }
            Some(ext)
        })
        .collect()
}

fn is_process_alive(pid_path: &Path) -> bool {
    let pid_str = match std::fs::read_to_string(pid_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pid: u32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::discover_sessions;
    use crate::test_utils::EnvGuard;
    use serde_json::Value;

    fn unique_socket_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "agent-browser-stream-discovery-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn test_discover_sessions_reads_json_stream_metadata() {
        let guard = EnvGuard::new(&["AGENT_BROWSER_SOCKET_DIR"]);
        let socket_dir = unique_socket_dir("json");
        std::fs::create_dir_all(&socket_dir).expect("socket dir should be created");
        guard.set(
            "AGENT_BROWSER_SOCKET_DIR",
            socket_dir.to_str().expect("socket dir should be utf-8"),
        );

        std::fs::write(socket_dir.join("json.pid"), std::process::id().to_string())
            .expect("pid file should be written");
        std::fs::write(
            socket_dir.join("json.stream"),
            r#"{"addr":"0.0.0.0","port":9223}"#,
        )
        .expect("stream file should be written");

        let sessions: Vec<Value> = serde_json::from_str(&discover_sessions())
            .expect("discovered sessions should be valid JSON");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["session"], "json");
        assert_eq!(sessions[0]["addr"], "0.0.0.0");
        assert_eq!(sessions[0]["port"], 9223);

        let _ = std::fs::remove_dir_all(&socket_dir);
    }

    #[test]
    fn test_discover_sessions_reads_legacy_port_only_metadata() {
        let guard = EnvGuard::new(&["AGENT_BROWSER_SOCKET_DIR"]);
        let socket_dir = unique_socket_dir("legacy");
        std::fs::create_dir_all(&socket_dir).expect("socket dir should be created");
        guard.set(
            "AGENT_BROWSER_SOCKET_DIR",
            socket_dir.to_str().expect("socket dir should be utf-8"),
        );

        std::fs::write(socket_dir.join("legacy.pid"), std::process::id().to_string())
            .expect("pid file should be written");
        std::fs::write(socket_dir.join("legacy.stream"), "9223")
            .expect("stream file should be written");

        let sessions: Vec<Value> = serde_json::from_str(&discover_sessions())
            .expect("discovered sessions should be valid JSON");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["addr"], "127.0.0.1");
        assert_eq!(sessions[0]["port"], 9223);

        let _ = std::fs::remove_dir_all(&socket_dir);
    }
}
