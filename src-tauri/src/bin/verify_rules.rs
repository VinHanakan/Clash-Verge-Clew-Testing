use app_lib::core::app_proxy::{AppProxyManager, AppProxyRule};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== MULTI-APPLICATION RULES & PERSISTENCE VERIFICATION ===");

    // 1. Verify AppProxyRule Schema & Serialization
    println!("\n1. Testing AppProxyRule serialization and schema compatibility...");
    let rule1 = AppProxyRule {
        id: "rule-edge-1".into(),
        name: "Microsoft Edge".into(),
        enabled: true,
        process_name: "*".into(),
        cmdline_pattern: "*msedge.exe*".into(),
        hack_tree: false,
        proxy_group_id: 0,
        strategy_group: "regular".into(),
        protocol: "tcp".into(),
    };

    let serialized = serde_json::to_string_pretty(&rule1)?;
    println!("-> Serialized rule:\n{serialized}");
    assert!(serialized.contains("\"id\": \"rule-edge-1\""));
    assert!(serialized.contains("\"cmdline_pattern\": \"*msedge.exe*\""));
    assert!(serialized.contains("\"enabled\": true"));
    assert!(serialized.contains("\"protocol\": \"tcp\""));

    let deserialized: AppProxyRule = serde_json::from_str(&serialized)?;
    assert_eq!(deserialized.id, rule1.id);
    assert_eq!(deserialized.name, rule1.name);
    assert_eq!(deserialized.enabled, rule1.enabled);
    assert_eq!(deserialized.cmdline_pattern, rule1.cmdline_pattern);

    // 2. Verify Schema Defaults for minimal JSON (e.g. from frontend)
    println!("\n2. Testing minimal JSON deserialization with defaults...");
    let minimal_json = r#"{
        "id": "rule-spotify",
        "name": "Spotify",
        "cmdline_pattern": "*Spotify.exe*"
    }"#;
    let minimal_rule: AppProxyRule = serde_json::from_str(minimal_json)?;
    assert_eq!(minimal_rule.id, "rule-spotify");
    assert_eq!(minimal_rule.process_name, "*");
    assert_eq!(minimal_rule.protocol, "tcp");
    assert!(!minimal_rule.hack_tree);
    assert_eq!(minimal_rule.proxy_group_id, 0);
    assert!(minimal_rule.enabled, "default rule should be enabled");
    println!("-> Minimal JSON deserialized with correct defaults: {:?}", minimal_rule);

    // 3. Verify Helper Protocol Payload Schema
    println!("\n3. Testing Clew helper rules payload construction...");
    let rule2 = AppProxyRule {
        id: "rule-chrome-2".into(),
        name: "Google Chrome".into(),
        enabled: false,
        process_name: "*".into(),
        cmdline_pattern: "chrome.exe".into(),
        hack_tree: false,
        proxy_group_id: 0,
        strategy_group: "regular".into(),
        protocol: "tcp".into(),
    };
    let rules_list = vec![rule1.clone(), rule2.clone()];
    let helper_payload = serde_json::json!({
        "expected_version": 3,
        "rules": rules_list
    });
    let payload_str = serde_json::to_string_pretty(&helper_payload)?;
    println!("-> Helper payload:\n{payload_str}");
    assert_eq!(helper_payload["expected_version"], 3);
    assert_eq!(helper_payload["rules"].as_array().unwrap().len(), 2);

    // 4. Verify AppProxyManager Rule Management In-Memory & Persistence
    println!("\n4. Testing AppProxyManager rule management operations...");
    let manager = AppProxyManager::default();

    // Reset rules for clean test
    let _ = manager.set_rules(vec![]).await;
    assert_eq!(manager.get_rules().len(), 0);

    // Add rule 1
    println!("-> Adding rule 1 (Edge)...");
    let _ = manager.add_or_update_rule(rule1.clone()).await;
    let current_rules = manager.get_rules();
    assert_eq!(current_rules.len(), 1);
    assert_eq!(current_rules[0].id, "rule-edge-1");
    assert!(current_rules[0].enabled);

    // Add rule 2
    println!("-> Adding rule 2 (Chrome)...");
    let _ = manager.add_or_update_rule(rule2.clone()).await;
    let current_rules = manager.get_rules();
    assert_eq!(current_rules.len(), 2);

    // Toggle rule 1 (disable)
    println!("-> Toggling rule 1 to disabled...");
    let _ = manager.toggle_rule("rule-edge-1", false).await;
    let current_rules = manager.get_rules();
    let edge_rule = current_rules.iter().find(|r| r.id == "rule-edge-1").unwrap();
    assert!(!edge_rule.enabled, "rule-edge-1 should now be disabled");

    // Toggle rule 1 (re-enable)
    println!("-> Toggling rule 1 to enabled...");
    let _ = manager.toggle_rule("rule-edge-1", true).await;
    let current_rules = manager.get_rules();
    let edge_rule = current_rules.iter().find(|r| r.id == "rule-edge-1").unwrap();
    assert!(edge_rule.enabled, "rule-edge-1 should now be enabled");

    // Remove rule 2
    println!("-> Removing rule 2 (Chrome)...");
    let _ = manager.remove_rule("rule-chrome-2").await;
    let current_rules = manager.get_rules();
    assert_eq!(current_rules.len(), 1);
    assert_eq!(current_rules[0].id, "rule-edge-1");

    // Verify a new manager instance loads the persisted rules from disk
    println!("-> Verifying fresh AppProxyManager instance reloads persisted rules from disk...");
    let fresh_manager = AppProxyManager::default();
    let reloaded_rules = fresh_manager.get_rules();
    assert_eq!(reloaded_rules.len(), 1);
    assert_eq!(reloaded_rules[0].id, "rule-edge-1");
    assert_eq!(reloaded_rules[0].cmdline_pattern, "*msedge.exe*");

    // Clean up test rule
    let _ = fresh_manager.set_rules(vec![]).await;
    assert_eq!(fresh_manager.get_rules().len(), 0);

    println!("\n=== PASS: ALL MULTI-APPLICATION RULES & PERSISTENCE CHECKS SUCCEEDED ===");
    Ok(())
}
