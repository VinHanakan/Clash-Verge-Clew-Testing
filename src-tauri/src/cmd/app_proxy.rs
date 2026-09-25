use super::{CmdResult, CommandFailure};
use crate::core::app_proxy::{
    AppProxyConnection, AppProxyManager, AppProxyProcessDetail, AppProxyProcessNode,
    AppProxyRule, AppProxyStartRequest, AppProxyStatus,
};
use clash_verge_logging::{Type, logging};
use tauri::State;

#[tauri::command]
pub async fn start_app_proxy(
    state: State<'_, AppProxyManager>,
    request: AppProxyStartRequest,
) -> CmdResult<crate::core::app_proxy::AppProxyStatus> {
    let correlation_id = request.correlation_id.clone().unwrap_or_else(|| "none".into());
    logging!(
        info,
        Type::Cmd,
        "app proxy command= start_app_proxy phase=entered correlation_id={correlation_id}"
    );
    match state.inner().start(request).await {
        Ok(result) => {
            logging!(
                info,
                Type::Cmd,
                "app proxy command=start_app_proxy phase=returned state={} proxy_ready={} effective_version={:?} correlation_id={correlation_id}",
                result.state,
                result.proxy_ready,
                result.effective_rules_version
            );
            Ok(result)
        }
        Err(error) => {
            logging!(
                error,
                Type::Cmd,
                "app proxy command=start_app_proxy phase=failed correlation_id={correlation_id} error={error:#}"
            );
            Err(CommandFailure::coded("APP_PROXY_START_FAILED", error))
        }
    }
}

#[tauri::command]
pub async fn stop_app_proxy(
    state: State<'_, AppProxyManager>,
    correlation_id: Option<String>,
) -> CmdResult<crate::core::app_proxy::AppProxyStatus> {
    let correlation_id = correlation_id.as_deref().unwrap_or("none");
    logging!(
        info,
        Type::Cmd,
        "app proxy command=stop_app_proxy phase=entered correlation_id={correlation_id}"
    );
    match state.inner().stop().await {
        Ok(result) => {
            logging!(
                info,
                Type::Cmd,
                "app proxy command=stop_app_proxy phase=returned state={} correlation_id={correlation_id}",
                result.state
            );
            Ok(result)
        }
        Err(error) => {
            logging!(
                error,
                Type::Cmd,
                "app proxy command=stop_app_proxy phase=failed correlation_id={correlation_id} error={error:#}"
            );
            Err(CommandFailure::coded("APP_PROXY_STOP_FAILED", error))
        }
    }
}

#[tauri::command]
pub async fn clear_app_proxy_error(
    state: State<'_, AppProxyManager>,
) -> CmdResult<()> {
    state.inner().clear_startup_error();
    Ok(())
}

#[tauri::command]
pub async fn get_app_proxy_status(
    state: State<'_, AppProxyManager>,
    correlation_id: Option<String>,
) -> CmdResult<AppProxyStatus> {
    let correlation_id = correlation_id.as_deref().unwrap_or("none");
    logging!(
        info,
        Type::Cmd,
        "app proxy command=get_app_proxy_status phase=entered correlation_id={correlation_id}"
    );
    let result = state.inner().status().await;
    logging!(
        info,
        Type::Cmd,
        "app proxy command=get_app_proxy_status phase=returned state={} helper_ready={} proxy_ready={} effective_version={:?} correlation_id={correlation_id}",
        result.state,
        result.helper_ready,
        result.proxy_ready,
        result.effective_rules_version
    );
    Ok(result)
}

#[tauri::command]
pub async fn set_app_proxy_strategy(
    state: State<'_, AppProxyManager>,
    strategy_group: String,
    correlation_id: Option<String>,
) -> CmdResult<AppProxyStatus> {
    let correlation_id = correlation_id.as_deref().unwrap_or("none");
    logging!(
        info,
        Type::Cmd,
        "app proxy command=set_app_proxy_strategy phase=entered group={} correlation_id={correlation_id}",
        strategy_group
    );
    let rules = state.inner().get_rules().into_iter().map(|mut rule| {
        rule.strategy_group = strategy_group.clone();
        rule
    }).collect();
    match state.inner().set_rules(rules).await {
        Ok(result) => {
            logging!(
                info,
                Type::Cmd,
                "app proxy command=set_app_proxy_strategy phase=returned state={} proxy_ready={} effective_version={:?} correlation_id={correlation_id}",
                result.state,
                result.proxy_ready,
                result.effective_rules_version
            );
            Ok(result)
        }
        Err(error) => {
            logging!(
                error,
                Type::Cmd,
                "app proxy command=set_app_proxy_strategy phase=failed correlation_id={correlation_id} error={error:#}"
            );
            Err(CommandFailure::coded("APP_PROXY_STRATEGY_FAILED", error))
        }
    }
}

#[tauri::command]
pub async fn get_app_proxy_connections(
    state: State<'_, AppProxyManager>,
) -> CmdResult<Vec<AppProxyConnection>> {
    match state.inner().get_connections().await {
        Ok(conns) => Ok(conns),
        Err(err) => {
            logging!(warn, Type::Cmd, "app proxy connections query failed: {err:#}");
            Err(CommandFailure::coded("GET_CONNECTIONS_FAILED", err))
        }
    }
}

#[tauri::command]
pub async fn get_app_proxy_process_tree(
    state: State<'_, AppProxyManager>,
) -> CmdResult<Vec<AppProxyProcessNode>> {
    match state.inner().get_process_tree().await {
        Ok(tree) => Ok(tree),
        Err(err) => {
            logging!(warn, Type::Cmd, "app proxy process tree query failed: {err:#}");
            Err(CommandFailure::coded("GET_PROCESS_TREE_FAILED", err))
        }
    }
}

#[tauri::command]
pub async fn get_app_proxy_process_detail(
    state: State<'_, AppProxyManager>,
    pid: u32,
) -> CmdResult<AppProxyProcessDetail> {
    match state.inner().get_process_detail(pid).await {
        Ok(detail) => Ok(detail),
        Err(err) => {
            logging!(warn, Type::Cmd, "app proxy process detail query failed pid={pid}: {err:#}");
            Err(CommandFailure::coded("GET_PROCESS_DETAIL_FAILED", err))
        }
    }
}

#[tauri::command]
pub async fn get_app_proxy_rules(
    state: State<'_, AppProxyManager>,
) -> CmdResult<Vec<AppProxyRule>> {
    Ok(state.inner().get_rules())
}

#[tauri::command]
pub async fn get_app_proxy_strategy_groups() -> CmdResult<Vec<String>> {
    crate::core::app_proxy::list_strategy_groups().await
        .map_err(|error| {
            logging!(warn, Type::Cmd, "app proxy strategy groups query failed: {error:#}");
            CommandFailure::coded("GET_APP_PROXY_GROUPS_FAILED", error)
        })
}

#[tauri::command]
pub async fn set_app_proxy_rules(
    state: State<'_, AppProxyManager>,
    rules: Vec<AppProxyRule>,
) -> CmdResult<AppProxyStatus> {
    match state.inner().set_rules(rules).await {
        Ok(status) => Ok(status),
        Err(err) => Err(CommandFailure::coded("SET_APP_PROXY_RULES_FAILED", err)),
    }
}

#[tauri::command]
pub async fn add_app_proxy_rule(
    state: State<'_, AppProxyManager>,
    rule: AppProxyRule,
) -> CmdResult<AppProxyStatus> {
    match state.inner().add_or_update_rule(rule).await {
        Ok(status) => Ok(status),
        Err(err) => Err(CommandFailure::coded("ADD_APP_PROXY_RULE_FAILED", err)),
    }
}

#[tauri::command]
pub async fn remove_app_proxy_rule(
    state: State<'_, AppProxyManager>,
    rule_id: String,
) -> CmdResult<AppProxyStatus> {
    match state.inner().remove_rule(&rule_id).await {
        Ok(status) => Ok(status),
        Err(err) => Err(CommandFailure::coded("REMOVE_APP_PROXY_RULE_FAILED", err)),
    }
}

#[tauri::command]
pub async fn toggle_app_proxy_rule(
    state: State<'_, AppProxyManager>,
    rule_id: String,
    enabled: bool,
) -> CmdResult<AppProxyStatus> {
    match state.inner().toggle_rule(&rule_id, enabled).await {
        Ok(status) => Ok(status),
        Err(err) => Err(CommandFailure::coded("TOGGLE_APP_PROXY_RULE_FAILED", err)),
    }
}
