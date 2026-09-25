#include "transport/http_api_server.hpp"

#include <chrono>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include "common/api_context.hpp"
#include "common/api_exception.hpp"
#include "services/config_service.hpp"
#include "core/log.hpp"
#include "transport/response_utils.hpp"
#include "transport/middleware.hpp"
#include "transport/route_registry.hpp"

// Handler modules define register_* functions in the ::clew namespace.
namespace clew {
void register_autostart_handlers(route_registry&);
void register_config_handlers(route_registry&);
void register_connection_handlers(route_registry&);
void register_group_handlers(route_registry&);
void register_icon_handlers(route_registry&);
void register_process_handlers(route_registry&);
void register_rule_handlers(route_registry&);
void register_shell_handlers(route_registry&);
void register_stats_handlers(route_registry&);
} // namespace clew

namespace clew::transport {

http_api_server::http_api_server(int port, api_context& ctx, std::string static_dir,
                                 bool require_write_auth, std::string api_token,
                                 std::function<nlohmann::json()> status_provider,
                                 std::function<void()> stop_provider)
    : port_(port)
    , static_dir_(std::move(static_dir))
    , require_write_auth_(require_write_auth)
    , api_token_(std::move(api_token))
    , status_provider_(std::move(status_provider))
    , stop_provider_(std::move(stop_provider))
    , ctx_(ctx) {
    // Thread pool sized for our actual load: 3 polling endpoints
    // (stats/tcp/udp) + occasional CRUD + icon fetches. Default would
    // be std::thread::hardware_concurrency(); 8 is a safe fixed cap
    // that doesn't depend on machine class. The earlier 32 was sized
    // for SSE long-lived connections, which the project no longer has.
    server_.new_task_queue = [] { return new httplib::ThreadPool(8); };

    install_default_headers(server_);
    install_options_handler(server_);
    install_cache_headers(server_);

    // In headless mode every API except the low-sensitivity capability
    // handshake is authenticated, regardless of HTTP verb.
    server_.set_pre_routing_handler([this](const httplib::Request& req,
                                           httplib::Response& res) {
        if (!require_write_auth_ || !req.path.starts_with("/api/") ||
            req.path == "/api/helper/v1/handshake") {
            return httplib::Server::HandlerResponse::Unhandled;
        }
        const auto auth = req.get_header_value("Authorization");
        const std::string expected = "Bearer " + api_token_;
        if (auth == expected) return httplib::Server::HandlerResponse::Unhandled;
        res.status = 401;
        res.set_content(R"({"error":"missing or invalid bearer token"})", "application/json");
        return httplib::Server::HandlerResponse::Handled;
    });

    server_.Get("/api/helper/v1/handshake", [this](const httplib::Request&, httplib::Response& res) {
        res.set_content(nlohmann::json{
            {"protocol_version", 1},
            {"helper", "clew"},
            {"headless", require_write_auth_},
        }.dump(), "application/json");
    });
    server_.Get("/api/helper/v1/status", [this](const httplib::Request&, httplib::Response& res) {
        res.set_content(status_provider_ ? status_provider_().dump() : R"({"helper_ready":false})",
                        "application/json");
    });
    server_.Get("/api/helper/v1/capabilities", [](const httplib::Request&, httplib::Response& res) {
        res.set_content(R"({"protocol_version":1,"capabilities":["handshake","status","replace_rules","stop","exit"]})", "application/json");
    });
    server_.Post("/api/helper/v1/stop", [this](const httplib::Request&, httplib::Response& res) {
        if (stop_provider_) stop_provider_();
        res.set_content(R"({"state":"stopping"})", "application/json");
    });
    server_.Post("/api/helper/v1/exit", [this](const httplib::Request&, httplib::Response& res) {
        if (stop_provider_) stop_provider_();
        res.set_content(R"({"state":"stopping"})", "application/json");
    });
    server_.Put("/api/helper/v1/rules", [this](const httplib::Request& req, httplib::Response& res) {
        try {
            auto body = parse_json_body(req);
            write_json(res, ctx_.config.replace_rules(body));
        } catch (const api_exception& e) {
            res.status = api_error_to_http_status(e.code());
            res.set_content(nlohmann::json{{"error", e.message()}}.dump(), "application/json");
        } catch (const std::exception& e) {
            res.status = 400;
            res.set_content(nlohmann::json{{"error", e.what()}}.dump(), "application/json");
        }
    });

    clew::route_registry reg(server_, ctx_);
    if (!require_write_auth_) clew::register_autostart_handlers(reg);
    clew::register_config_handlers(reg);
    clew::register_connection_handlers(reg);
    clew::register_group_handlers(reg);
    clew::register_icon_handlers(reg);
    clew::register_process_handlers(reg);
    clew::register_rule_handlers(reg);
    if (!require_write_auth_) clew::register_shell_handlers(reg);
    clew::register_stats_handlers(reg);

    if (!require_write_auth_) setup_static_files();
}

http_api_server::~http_api_server() {
    stop();
}

bool http_api_server::start() {
    if (running_.exchange(true)) return true;

    server_thread_ = std::jthread([this]() {
        PC_LOG_INFO("[api] HTTP API server listening on 127.0.0.1:{}", port_);
        if (!server_.listen("127.0.0.1", port_)) {
            PC_LOG_ERROR("[api] failed to bind port {}", port_);
            running_ = false;
        }
    });
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    return running_;
}

void http_api_server::stop() {
    if (!running_.exchange(false)) return;
    server_.stop();
    if (server_thread_.joinable()) {
        server_thread_.join();
    }
    PC_LOG_INFO("[api] HTTP API server stopped");
}

std::string http_api_server::get_executable_dir() {
    char path[MAX_PATH];
    if (DWORD len = GetModuleFileNameA(nullptr, path, MAX_PATH); len > 0 && len < MAX_PATH) {
        std::string_view sv{path, static_cast<std::size_t>(len)};
        if (auto last_sep = sv.find_last_of("\\/"); last_sep != std::string_view::npos) {
            return std::string(sv.substr(0, last_sep));
        }
    }
    return ".";
}

void http_api_server::setup_static_files() {
    // All candidates are anchored to either an explicit --static-dir or
    // the exe directory. We deliberately do NOT include cwd-relative
    // paths ("./frontend/dist" etc.) anymore — Task Scheduler launches
    // clew.exe with cwd=system32 and Explorer double-click uses the
    // desktop folder, neither of which is reachable from clew's resources.
    // The two exe_dir candidates cover release zip layout
    // (exe_dir/frontend/dist) and dev build layout
    // (build/Release/clew.exe → exe_dir/../../frontend/dist).
    const std::vector<std::string> candidates = {
        static_dir_,
        get_executable_dir() + "/frontend/dist",
        get_executable_dir() + "/../../frontend/dist",
    };

    for (const auto& path : candidates) {
        if (path.empty()) continue;
        std::string index_path = path + "/index.html";
        if (GetFileAttributesA(index_path.c_str()) != INVALID_FILE_ATTRIBUTES) {
            if (server_.set_mount_point("/", path)) {
                PC_LOG_INFO("[api] serving static files from {}", path);
                return;
            }
        }
    }
    PC_LOG_WARN("[api] static files directory not found; web UI unavailable");
}

} // namespace clew::transport
