// 参考 since2006/shell-rs @ b2a307a: src/host/quick_connect.rs 的 `QuickHost::matches` 重写：
// 搜索范围按 QiShell 的 spec 只有名称、地址、用户名（不查备注和分组）。

//! 主机栏和快速连接共用的主机搜索。

use crate::Host;

/// `host` 是否匹配搜索框里的 `query`：按空白切成几个词，每个词都要出现在名称、地址或
/// 用户名里（不分大小写）。空的 `query` 匹配全部主机。
pub fn host_matches(host: &Host, query: &str) -> bool {
    let fields = [&host.name, &host.address, &host.user].map(|field| field.to_lowercase());
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| fields.iter().any(|field| field.contains(word)))
}

#[cfg(test)]
mod tests {
    use super::host_matches;
    use crate::{AuthKind, Host, HostDraft, HostId};

    fn host() -> Host {
        Host::new(
            HostId(1),
            HostDraft::new("Web-01", "10.0.1.12", 22, "deploy", AuthKind::Password)
                .with_notes("生产 nginx"),
        )
    }

    #[test]
    fn an_empty_query_matches_every_host() {
        assert!(host_matches(&host(), ""));
        assert!(host_matches(&host(), "   "));
    }

    #[test]
    fn a_word_matches_the_name_the_address_or_the_user_whatever_the_case() {
        for query in ["web", "WEB-0", "10.0.1", "deploy", "DEP"] {
            assert!(host_matches(&host(), query), "{query}");
        }
        assert!(!host_matches(&host(), "db"));
    }

    #[test]
    fn every_word_has_to_match_somewhere() {
        assert!(host_matches(&host(), "web deploy"));
        assert!(host_matches(&host(), " 10.0 01 "));
        assert!(!host_matches(&host(), "web root"));
    }

    #[test]
    fn notes_are_not_searched() {
        assert!(!host_matches(&host(), "nginx"));
    }
}
