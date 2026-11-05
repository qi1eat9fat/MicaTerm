//! 凭据（工单 14）的夹具：种子主机加四条凭据，像原型的凭据面板那样有人在用、有人没人用。

use qishell_hosts::{
    CredentialDraft, CredentialId, CredentialKind, GeneratedKey, HostData, HostId, KeyAlgorithm,
};

/// 种子数据加四条凭据（id 依次 1 到 4）：
/// 1. 「部署密钥」：密钥（Ed25519），deploy，db-01（3）和 staging-api（4）在用；
/// 2. 「运维密码」：密码，root，web-01（1）在用；
/// 3. 「本机 Agent」：SSH Agent，ops，qa-runner（5）在用；
/// 4. 「旧 RSA 密钥」：密钥（读不出公钥），admin，没人用。
pub fn seed_with_credentials() -> HostData {
    let mut data = HostData::seed();
    let key = GeneratedKey::generate(KeyAlgorithm::Ed25519).expect("a key");
    let deploy = data.insert_credential_unnotified(
        CredentialDraft::new("部署密钥", CredentialKind::Key, "deploy")
            .with_public_key(key.public_key_line("deploy@qishell")),
    );
    let ops = data.insert_credential_unnotified(CredentialDraft::new(
        "运维密码",
        CredentialKind::Password,
        "root",
    ));
    let agent = data.insert_credential_unnotified(CredentialDraft::new(
        "本机 Agent",
        CredentialKind::Agent,
        "ops",
    ));
    data.insert_credential_unnotified(CredentialDraft::new(
        "旧 RSA 密钥",
        CredentialKind::Key,
        "admin",
    ));
    for (host, credential) in [(3, deploy), (4, deploy), (1, ops), (5, agent)] {
        use_credential(&mut data, host, credential);
    }
    data
}

/// 让主机 `host` 用凭据 `credential` 登录。
pub fn use_credential(data: &mut HostData, host: u64, credential: CredentialId) {
    let draft = data
        .host(HostId(host))
        .expect("a seeded host")
        .draft()
        .with_credential(credential);
    data.update_unnotified(HostId(host), draft);
}
