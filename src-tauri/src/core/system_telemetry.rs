//! Native system telemetry provider for process observation and TCP connection monitoring.
//!
//! Queries Windows APIs (Toolhelp32 snapshot, IP Helper GetExtendedTcpTable) directly
//! without requiring elevated privileges, WinDivert drivers, Mihomo listeners, or external helpers.

use anyhow::Result;
use super::app_proxy::{AppProxyConnection, AppProxyProcessDetail, AppProxyProcessNode};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::{
        collections::HashMap,
        ffi::OsString,
        mem::{size_of, zeroed},
        net::Ipv4Addr,
        os::windows::ffi::OsStringExt,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
        },
        Networking::WinSock::AF_INET,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };

    fn tcp_state_to_str(state: u32) -> &'static str {
        match state {
            1 => "CLOSED",
            2 => "LISTEN",
            3 => "SYN_SENT",
            4 => "SYN_RCVD",
            5 => "ESTABLISHED",
            6 => "FIN_WAIT1",
            7 => "FIN_WAIT2",
            8 => "CLOSE_WAIT",
            9 => "CLOSING",
            10 => "LAST_ACK",
            11 => "TIME_WAIT",
            12 => "DELETE_TCB",
            _ => "UNKNOWN",
        }
    }

    struct RawProcessInfo {
        pid: u32,
        parent_pid: u32,
        name: String,
        image_path: Option<String>,
    }

    fn query_process_image_path(pid: u32) -> Option<String> {
        if pid == 0 || pid == 4 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut path_buf = [0u16; 1024];
            let mut len = path_buf.len() as u32;
            let res = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, path_buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if res != 0 && len > 0 {
                let os_str = OsString::from_wide(&path_buf[..len as usize]);
                os_str.into_string().ok()
            } else {
                None
            }
        }
    }

    fn snapshot_processes() -> HashMap<u32, RawProcessInfo> {
        let mut map = HashMap::new();
        unsafe {
            let snapshot: HANDLE = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
                return map;
            }

            let mut entry: PROCESSENTRY32W = zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let pid = entry.th32ProcessID;
                    let parent_pid = entry.th32ParentProcessID;

                    // Decode null-terminated wide string for executable name
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = OsString::from_wide(&entry.szExeFile[..len])
                        .to_string_lossy()
                        .into_owned();

                    let image_path = query_process_image_path(pid);

                    map.insert(
                        pid,
                        RawProcessInfo {
                            pid,
                            parent_pid,
                            name,
                            image_path,
                        },
                    );

                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
        map
    }

    pub fn get_system_tcp_connections() -> Result<Vec<AppProxyConnection>> {
        let proc_map = snapshot_processes();
        let mut connections = Vec::new();

        unsafe {
            let mut size: u32 = 0;
            let ret = GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if ret != ERROR_INSUFFICIENT_BUFFER as u32 && ret != NO_ERROR {
                return Ok(connections);
            }

            let mut buffer = vec![0u8; size as usize];
            let ret = GetExtendedTcpTable(
                buffer.as_mut_ptr() as *mut _,
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if ret != NO_ERROR {
                return Ok(connections);
            }

            let table = &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let num_entries = table.dwNumEntries as usize;
            let entries_ptr = table.table.as_ptr() as *const MIB_TCPROW_OWNER_PID;

            for i in 0..num_entries {
                let row = &*entries_ptr.add(i);
                let pid = row.dwOwningPid;

                let local_ip = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
                let local_port = u16::from_be(row.dwLocalPort as u16);
                let remote_ip = Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes());
                let remote_port = u16::from_be(row.dwRemotePort as u16);
                let state = tcp_state_to_str(row.dwState).to_string();

                let process_name = proc_map
                    .get(&pid)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| {
                        if pid == 0 {
                            "System Idle Process".into()
                        } else if pid == 4 {
                            "System".into()
                        } else {
                            format!("PID {pid}")
                        }
                    });

                let dest = format!("{remote_ip}:{remote_port}");

                connections.push(AppProxyConnection {
                    pid,
                    process_name,
                    local_ip: local_ip.to_string(),
                    local_port,
                    remote_ip: remote_ip.to_string(),
                    remote_port,
                    state,
                    dest: Some(dest),
                    hijacked: false,
                    pid_alive: proc_map.contains_key(&pid) || pid == 0 || pid == 4,
                    proxy_status: Some("DIRECT".into()),
                });
            }
        }

        Ok(connections)
    }

    pub fn get_system_process_tree() -> Result<Vec<AppProxyProcessNode>> {
        let proc_map = snapshot_processes();

        // Group children by parent PID
        let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();
        for proc in proc_map.values() {
            children_map.entry(proc.parent_pid).or_default().push(proc.pid);
        }

        fn build_node(
            pid: u32,
            proc_map: &HashMap<u32, RawProcessInfo>,
            children_map: &HashMap<u32, Vec<u32>>,
            visited: &mut HashMap<u32, bool>,
        ) -> Option<AppProxyProcessNode> {
            if visited.insert(pid, true) == Some(true) {
                // Cycle detected
                return None;
            }

            let proc = proc_map.get(&pid)?;
            let mut children = Vec::new();
            if let Some(child_pids) = children_map.get(&pid) {
                for &child_pid in child_pids {
                    if let Some(child_node) = build_node(child_pid, proc_map, children_map, visited) {
                        children.push(child_node);
                    }
                }
            }

            // Sort children by name and PID
            children.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.pid.cmp(&b.pid)));

            Some(AppProxyProcessNode {
                pid: proc.pid,
                parent_pid: proc.parent_pid,
                name: proc.name.clone(),
                hijacked: false,
                hijack_source: None,
                children,
                cmdline: None,
                image_path: proc.image_path.clone(),
            })
        }

        // Roots are processes whose parent_pid is 0, or whose parent_pid does not exist in proc_map
        let mut roots = Vec::new();
        let mut visited = HashMap::new();

        let mut root_pids: Vec<u32> = proc_map
            .values()
            .filter(|p| p.parent_pid == 0 || !proc_map.contains_key(&p.parent_pid) || p.pid == p.parent_pid)
            .map(|p| p.pid)
            .collect();
        root_pids.sort();

        for pid in root_pids {
            if let Some(node) = build_node(pid, &proc_map, &children_map, &mut visited) {
                roots.push(node);
            }
        }

        // Any orphan processes not visited (due to cycles or disconnected components)
        for &pid in proc_map.keys() {
            if !visited.contains_key(&pid) {
                if let Some(node) = build_node(pid, &proc_map, &children_map, &mut visited) {
                    roots.push(node);
                }
            }
        }

        roots.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.pid.cmp(&b.pid)));
        Ok(roots)
    }

    pub fn get_system_process_detail(pid: u32) -> Result<AppProxyProcessDetail> {
        let proc_map = snapshot_processes();
        let proc = proc_map.get(&pid);
        let name = proc.map(|p| p.name.clone()).unwrap_or_else(|| format!("PID {pid}"));
        let parent_pid = proc.map(|p| p.parent_pid);
        let image_path = proc.and_then(|p| p.image_path.clone()).or_else(|| query_process_image_path(pid));
        let cmdline = None;

        Ok(AppProxyProcessDetail {
            pid,
            name,
            parent_pid,
            hijacked: false,
            hijack_source: None,
            cmdline,
            image_path,
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn get_system_tcp_connections() -> Result<Vec<AppProxyConnection>> {
        Ok(Vec::new())
    }

    pub fn get_system_process_tree() -> Result<Vec<AppProxyProcessNode>> {
        Ok(Vec::new())
    }

    pub fn get_system_process_detail(pid: u32) -> Result<AppProxyProcessDetail> {
        Ok(AppProxyProcessDetail {
            pid,
            name: format!("PID {pid}"),
            parent_pid: None,
            hijacked: false,
            hijack_source: None,
            cmdline: None,
            image_path: None,
        })
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_telemetry_queries() {
        let conns = get_system_tcp_connections().expect("query tcp connections");
        let tree = get_system_process_tree().expect("query process tree");

        println!("Found {} active TCP connections", conns.len());
        println!("Found {} root processes in process tree", tree.len());

        #[cfg(windows)]
        {
            assert!(!tree.is_empty(), "system process tree should not be empty");
            // Check that all connections have hijacked = false when queried from telemetry
            for c in &conns {
                assert!(!c.hijacked, "unmanaged connection should have hijacked = false");
                assert_eq!(c.proxy_status.as_deref(), Some("DIRECT"));
            }
        }
    }
}
