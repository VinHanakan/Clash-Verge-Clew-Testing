use app_lib::core::system_telemetry::{
    get_system_process_detail, get_system_process_tree, get_system_tcp_connections,
};

fn main() -> anyhow::Result<()> {
    println!("=== NATIVE TELEMETRY VERIFICATION ===");

    // 1. Verify TCP connections
    println!("\n1. Querying active system TCP connections...");
    let conns = get_system_tcp_connections()?;
    println!("-> Total active TCP connections found: {}", conns.len());
    assert!(!conns.is_empty(), "system should have active TCP connections");

    let hijacked_conns = conns.iter().filter(|c| c.hijacked).count();
    let direct_conns = conns.iter().filter(|c| c.proxy_status.as_deref() == Some("DIRECT")).count();
    println!("-> Hijacked connections: {hijacked_conns} (expected 0)");
    println!("-> Direct connections: {direct_conns} (expected {})", conns.len());
    assert_eq!(hijacked_conns, 0, "no connections should be hijacked in read-only mode");
    assert_eq!(direct_conns, conns.len(), "all connections should report DIRECT in read-only mode");

    if let Some(sample) = conns.first() {
        println!("-> Sample connection: PID {} ({}) {} -> {} [state={}, hijacked={}, status={:?}]",
            sample.pid, sample.process_name, sample.local_ip, sample.remote_ip, sample.state, sample.hijacked, sample.proxy_status);
    }

    // 2. Verify Process Tree
    println!("\n2. Querying system process tree...");
    let tree = get_system_process_tree()?;
    println!("-> Total root processes found: {}", tree.len());
    assert!(!tree.is_empty(), "process tree roots should not be empty");

    let mut total_nodes = 0;
    let mut hijacked_nodes = 0;
    let mut named_nodes = 0;
    let mut has_path_nodes = 0;

    fn walk_nodes(
        nodes: &[app_lib::core::app_proxy::AppProxyProcessNode],
        total: &mut usize,
        hijacked: &mut usize,
        named: &mut usize,
        has_path: &mut usize,
    ) {
        for node in nodes {
            *total += 1;
            if node.hijacked {
                *hijacked += 1;
            }
            if !node.name.is_empty() {
                *named += 1;
            }
            if node.image_path.is_some() {
                *has_path += 1;
            }
            walk_nodes(&node.children, total, hijacked, named, has_path);
        }
    }
    walk_nodes(&tree, &mut total_nodes, &mut hijacked_nodes, &mut named_nodes, &mut has_path_nodes);

    println!("-> Total processes observed: {total_nodes}");
    println!("-> Hijacked processes: {hijacked_nodes} (expected 0)");
    println!("-> Processes with valid names: {named_nodes}");
    println!("-> Processes with resolved image paths: {has_path_nodes}");
    assert!(total_nodes > 10, "should observe more than 10 processes in system");
    assert_eq!(hijacked_nodes, 0, "no processes should be hijacked in read-only mode");

    // 3. Verify Process Detail
    println!("\n3. Querying process detail for current PID ({})...", std::process::id());
    let current_pid = std::process::id();
    let detail = get_system_process_detail(current_pid)?;
    println!("-> Current process detail: PID={}, Name={}, ParentPID={:?}, ImagePath={:?}, Hijacked={}",
        detail.pid, detail.name, detail.parent_pid, detail.image_path, detail.hijacked);
    assert_eq!(detail.pid, current_pid);
    assert!(!detail.hijacked);

    println!("\n=== PASS: ALL NATIVE TELEMETRY CHECKS SUCCEEDED ===");
    Ok(())
}
