//! The official SDK client talks to the executable, not an in-process handler.
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::{Value, json};
use std::{fs, time::Duration};

#[tokio::test]
async fn official_client_initializes_lists_and_calls_every_read_tool_with_fresh_safe_facts() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        let state = root.path().join("state");
        fs::create_dir_all(project.join(".git")).unwrap();
        let folder = home.join(".agents/skills/example");
        fs::create_dir_all(&folder).unwrap();
        let skill = b"---\nname: example\ndescription: Searchable fixture\n---\nBODY_CANARY";
        fs::write(folder.join("SKILL.md"), skill).unwrap();
        std::os::unix::fs::symlink(root.path().join("outside"), folder.join("unresolved")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let marker = root.path().join("must-not-execute");
        let runner = root.path().join("runner.sh");
        fs::write(&runner, format!("#!/bin/sh\nprintf executed > '{}'\n", marker.display())).unwrap();
        let config = serde_json::to_vec(&json!({"mcpServers":{
            "test":{"command":"sh", "args":[runner], "env":{"PASSWORD":"CREDENTIAL_CANARY"}},
            "web":{"type":"http", "url":format!("http://{}/SECRET_URL", listener.local_addr().unwrap()),"headers":{"Authorization":"ARG_SECRET"}}
        }})).unwrap();
        fs::write(home.join(".claude.json"), &config).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_skillrail"));
        command.arg("--home").arg(&home).arg("--project").arg(&project).arg("--state-dir").arg(&state).arg("mcp");
        let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        let tools = client.list_tools(None).await.unwrap();
        let names: Vec<_> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, ["list_inventory", "search_inventory", "get_item", "explain_visibility", "list_drift"]);
        assert!(tools.tools.iter().all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true)));
        assert!(client.peer_info().unwrap().capabilities.resources.is_none());
        let list = client.call_tool(CallToolRequestParams::new("list_inventory")).await.unwrap();
        let value: Value = serde_json::from_str(&list.content[0].as_text().unwrap().text).unwrap();
        let id = value["items"].as_array().unwrap().iter().find(|i| i["kind"] == "skill").unwrap()["id"].as_str().unwrap();
        for (name, args) in [
            ("list_inventory", json!({})),
            ("search_inventory", json!({"query":"SEARCHABLE", "agent":"codex"})),
            ("get_item", json!({"id":id})),
            ("explain_visibility", json!({"id":id})),
            ("list_drift", json!({})),
        ] {
            let result = client.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())).await.unwrap();
            assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
            let text = &result.content[0].as_text().unwrap().text;
            for canary in ["BODY_CANARY", "NEVER_EXECUTE_CANARY", "ARG_SECRET", "CREDENTIAL_CANARY", "SECRET_URL"] { assert!(!text.contains(canary)); }
            let result: Value = serde_json::from_str(text).unwrap();
            assert_eq!(result["schema_version"], 1);
            assert!(!result["items"].as_array().unwrap().is_empty());
            if name == "list_drift" { assert_eq!(result["items"][0]["drift"], "unknown"); }
        }
        let unknown = client.call_tool(CallToolRequestParams::new("get_item").with_arguments(json!({"id":"missing"}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(unknown.is_error, Some(true));
        assert!(unknown.content[0].as_text().unwrap().text.contains("item_not_found"));
        for args in [json!({"limit":0}), json!({"kind":"bad"}), json!({"extra":"CREDENTIAL_CANARY"}), json!({"offset":1}), json!({"limit":"1"}), json!({"kind":null}), json!({"limit":-1})] {
            assert!(client.call_tool(CallToolRequestParams::new("list_inventory").with_arguments(args.as_object().unwrap().clone())).await.is_err());
        }
        let first = client.call_tool(CallToolRequestParams::new("list_inventory")
            .with_arguments(json!({"limit":1}).as_object().unwrap().clone())).await.unwrap();
        let first: Value = serde_json::from_str(&first.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(first["page"]["next_offset"], 1);
        let second = client.call_tool(CallToolRequestParams::new("list_inventory")
            .with_arguments(json!({"limit":1,"offset":1,"revision":first["page"]["revision"]}).as_object().unwrap().clone())).await.unwrap();
        let second: Value = serde_json::from_str(&second.content[0].as_text().unwrap().text).unwrap();
        assert!(first["items"][0]["id"].as_str().unwrap() < second["items"][0]["id"].as_str().unwrap());
        let added = home.join(".agents/skills/added"); fs::create_dir_all(&added).unwrap();
        fs::write(added.join("SKILL.md"), "---\nname: added\ndescription: new\n---\n").unwrap();
        let stale = client.call_tool(CallToolRequestParams::new("list_inventory")
            .with_arguments(json!({"offset":1,"revision":first["page"]["revision"]}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(stale.is_error, Some(true));
        assert!(stale.content[0].as_text().unwrap().text.contains("inventory_changed"));
        let fresh = client.call_tool(CallToolRequestParams::new("list_inventory")).await.unwrap();
        let fresh: Value = serde_json::from_str(&fresh.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(fresh["items"].as_array().unwrap().len(), value["items"].as_array().unwrap().len() + 1);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_skillrail"))
            .arg("--home").arg(&home).arg("--project").arg(&project).arg("--state-dir").arg(&state).args(["list", "--json"]).output().unwrap();
        let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(fresh["items"], cli["items"]); assert_eq!(fresh["occurrences"], cli["occurrences"]);
        assert_eq!(fs::read(folder.join("SKILL.md")).unwrap(), skill);
        assert_eq!(fs::read(home.join(".claude.json")).unwrap(), config);
        assert!(!marker.exists(), "inventory executed a configured command");
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock, "inventory contacted a configured endpoint");
        let cache = fs::read(state.join("index.sqlite3")).unwrap();
        assert!(!cache.windows(b"CREDENTIAL_CANARY".len()).any(|w| w == b"CREDENTIAL_CANARY"));
        client.cancel().await.unwrap();
    }).await.expect("MCP client timed out");
}
