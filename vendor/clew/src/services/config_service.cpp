#include "services/config_service.hpp"

#include "config/config_store.hpp"
#include "config/types.hpp"
#include "common/api_exception.hpp"

namespace clew {

config_service::config_service(config_store& cfg) : cfg_(cfg) {}

uint64_t config_service::rules_revision() const noexcept {
    std::scoped_lock lock(rules_mutex_);
    return rules_revision_;
}

std::string config_service::get_raw() const {
    return cfg_.raw_json();
}

void config_service::replace_raw(std::string_view raw) {
    if (headless_) {
        const auto parsed = nlohmann::json::parse(raw);
        if (parsed.value("dns", nlohmann::json::object()).value("enabled", false))
            throw api_exception{api_error::invalid_argument, "headless mode forbids dns.enabled"};
    }
    cfg_.replace_from_json(raw);
}

nlohmann::json config_service::replace_rules(const nlohmann::json& request) {
    std::scoped_lock lock(rules_mutex_);
    if (!request.is_object() || !request.contains("rules") || !request["rules"].is_array())
        throw api_exception{api_error::invalid_argument, "rules must be an array"};
    auto current = nlohmann::json::parse(cfg_.raw_json());
    const auto expected = request.value("expected_version", uint64_t{0});
    const auto observed = rules_revision_;
    if (expected != 0 && expected != observed)
        throw api_exception{api_error::conflict, "rules version conflict"};
    current["auto_rules"] = request["rules"];
    if (request.contains("proxy_groups")) {
        if (!request["proxy_groups"].is_array())
            throw api_exception{api_error::invalid_argument, "proxy_groups must be an array"};
        current["proxy_groups"] = request["proxy_groups"];
    }
    // Parse before replacing so malformed rules never displace the old config.
    try {
        (void)current.get<ConfigV2>();
    } catch (const nlohmann::json::exception& e) {
        throw api_exception{api_error::invalid_argument, std::string{"invalid rules: "} + e.what()};
    }
    cfg_.replace_from_json(current.dump());
    const auto effective = ++rules_revision_;
    // Every accepted replacement is a new revision. The protocol deliberately
    // does not claim idempotency; callers serialize updates and retain this
    // returned effective version for the next expected_version.
    return nlohmann::json{{"effective_version", effective}};
}

} // namespace clew
