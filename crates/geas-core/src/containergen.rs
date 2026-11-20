// SPDX-License-Identifier: GPL-3.0-or-later
//! コンテナ層と基盤層（infra vm_docker）の宣言から、Dockerfile・compose.yaml・.dockerignore・シークレットの見本を生成する。
//! 置き場所は `geas rust --host --runtime` の出力フォルダ（Dockerfile はそこを文脈にして、サーバーを組む）。
//! 自由な設定は書かせない：ここで決まるのは、宣言した値だけ。宣言していない口・パスは、出力に現れない。
use crate::ast::Program;
use crate::envdoctor::secret_var;

/// 生成する非 root ユーザーの番号。ホストのデータフォルダは、この番号が書けるようにしておく
pub const UID: u32 = 10001;

pub fn generate(p: &Program) -> Result<Vec<(String, String)>, String> {
    let c = p.container.as_ref().ok_or("container の宣言がありません")?;
    let i = p.infra.as_ref().ok_or("infra の宣言がありません")?;
    let db = format!("{}/app.sqlite", c.data.trim_end_matches('/'));
    let mut args: Vec<String> = vec!["--host".into(), "0.0.0.0".into(), "--port".into(), c.port.to_string(), "--db".into(), db, "--pwa".into(), "/app/ui".into()];
    if !i.host_name.is_empty() {
        args.push("--allow-host".into());
        args.push(i.host_name.clone());
    }
    let entry = std::iter::once("/usr/local/bin/geas_server".to_string()).chain(args).map(|a| format!("\"{}\"", a)).collect::<Vec<_>>().join(", ");
    let health = format!("exec 3<>/dev/tcp/127.0.0.1/{}; printf 'GET /api/health HTTP/1.0\\r\\n\\r\\n' >&3; head -1 <&3 | grep -q ' 200 '", c.port);
    let dockerfile = format!(
        "# geas が container の宣言から生成した（手で直さない）。文脈は `geas rust --host --runtime` の出力フォルダ\n\
FROM rust:1-slim AS build\n\
WORKDIR /src\n\
COPY . .\n\
RUN cd server && cargo build --release\n\
\n\
FROM debian:stable-slim\n\
RUN useradd --system --uid {uid} --no-create-home geas \\\n && mkdir -p {data} && chown {uid}:{uid} {data}\n\
COPY --from=build /src/server/target/release/geas_server /usr/local/bin/geas_server\n\
COPY ui /app/ui\n\
USER {uid}\n\
VOLUME [\"{data}\"]\n\
EXPOSE {port}\n\
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD [\"bash\", \"-c\", \"{health}\"]\n\
ENTRYPOINT [{entry}]\n",
        uid = UID,
        data = c.data,
        port = c.port,
        health = health.replace('\\', "\\\\").replace('"', "\\\""),
        entry = entry,
    );
    let ports = if i.via == "direct" { format!("{}:{}", i.host_port, c.port) } else { format!("127.0.0.1:{}:{}", c.port, c.port) };
    let mut compose = format!(
        "# geas が infra vm_docker の宣言から生成した（手で直さない）。「{name}」\n\
# 入れ替えは `docker compose up -d --build`。古いコンテナを止めてから新しいものを起動するので、新旧は重ならない（台数は 1）。\n\
# ホストのデータフォルダ {vol} は、番号 {uid} のユーザーが書ける状態にしておく（人の保証）。\n",
        name = i.name,
        vol = i.volume_host,
        uid = UID
    );
    if i.via == "proxy" {
        compose += &format!("# 前段のプロキシが、ホストの {} で受けて、127.0.0.1:{} へ渡す（プロキシと TLS は人の保証）。\n", i.host_port, c.port);
    }
    compose += &format!(
        "services:\n  app:\n    build: .\n    restart: unless-stopped\n    user: \"{uid}:{uid}\"\n    read_only: true\n    tmpfs:\n      - /tmp\n    cap_drop:\n      - ALL\n    security_opt:\n      - no-new-privileges:true\n    ports:\n      - \"{ports}\"\n    volumes:\n      - \"{vol}:{data}\"\n",
        uid = UID,
        ports = ports,
        vol = i.volume_host,
        data = c.data
    );
    if !c.secrets.is_empty() {
        compose += &format!("    env_file:\n      - \"{}\"\n", i.env_file);
    }
    let dockerignore = "# geas が生成した\ntarget\nserver/target\ntests\ndev\next\nmcp\n*.sqlite*\n".to_string();
    let mut out = vec![("Dockerfile".to_string(), dockerfile), ("compose.yaml".to_string(), compose), (".dockerignore".to_string(), dockerignore)];
    if !c.secrets.is_empty() {
        let body: String = c.secrets.iter().map(|s| format!("{}=\n", secret_var(s))).collect();
        out.push(("secrets.env.example".to_string(), format!("# 値は人が入れる。geas は値を持たない。置き場所は {}（所有者だけが読める権限にする）\n{}", i.env_file, body)));
    }
    Ok(out)
}

/// 仕様書の「コンテナと基盤」の表（項目・内容・由来）。宣言が無ければ空
pub fn rows(p: &Program) -> Vec<[String; 3]> {
    let (Some(c), Some(i)) = (p.container.as_ref(), p.infra.as_ref()) else { return vec![] };
    let r = |a: &str, b: String, d: &str| [a.to_string(), b, d.to_string()];
    let mut out = vec![
        r("基盤", format!("{}「{}」", i.kind, i.name), "infra"),
        r("コンテナの待ち受け口", c.port.to_string(), "container"),
        r("データのボリューム（コンテナの中）", format!("{}（保存先のファイルは {}/app.sqlite）", c.data, c.data.trim_end_matches('/')), "container"),
        r("データのボリューム（ホストの側）", i.volume_host.clone(), "infra"),
    ];
    out.push(if i.via == "proxy" {
        r("公開", format!("ホストの {} を前段のプロキシが受け、127.0.0.1:{} へ渡す。コンテナの口はホストの手元にだけ出す（プロキシと TLS は人の保証）", i.host_port, c.port), "infra")
    } else {
        r("公開", format!("ホストの {} にコンテナの口 {} を直に出す", i.host_port, c.port), "infra")
    });
    if !i.host_name.is_empty() {
        out.push(r("利用者が開く名前", format!("{}（サーバーが Host として受ける）", i.host_name), "infra"));
    }
    if !c.secrets.is_empty() {
        out.push(r("シークレット", format!("{}（値は {} から環境変数で受ける。geas は値を持たない）", c.secrets.iter().map(|s| secret_var(s)).collect::<Vec<_>>().join("・"), i.env_file), "container／infra"));
    }
    out.push(r("同時に動く台数", "1（入れ替えは、古いコンテナを止めてから新しいものを起動する）".into(), "infra"));
    out.push(r("実行の形", format!("非 root（番号 {}）・ルートは読み取り専用・権限は全部外す", UID), "geas が固定"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn prog(via: &str, secrets: &[&str]) -> Program {
        let mut p = Program::default();
        p.container = Some(ContainerDecl { port: 8080, data: "/var/lib/app".into(), secrets: secrets.iter().map(|s| s.to_string()).collect(), line: 1 });
        p.infra = Some(InfraDecl { kind: "vm_docker".into(), name: "店".into(), host_port: 443, via: via.into(), host_name: "shop.example.com".into(), volume_host: "/srv/shop/data".into(), env_file: "/etc/shop/secrets.env".into(), line: 2 });
        p
    }

    fn get<'a>(f: &'a [(String, String)], n: &str) -> &'a str {
        &f.iter().find(|x| x.0 == n).unwrap().1
    }

    #[test]
    fn proxy_はホストの手元の口にだけ出す() {
        let f = generate(&prog("proxy", &["Payment.api_key"])).unwrap();
        let c = get(&f, "compose.yaml");
        assert!(c.contains("\"127.0.0.1:8080:8080\""));
        assert!(c.contains("\"/srv/shop/data:/var/lib/app\""));
        assert!(c.contains("/etc/shop/secrets.env"));
        assert!(get(&f, "secrets.env.example").contains("GEAS_SECRET_PAYMENT_API_KEY=\n"));
        let d = get(&f, "Dockerfile");
        assert!(d.contains("\"--host\", \"0.0.0.0\""));
        assert!(d.contains("\"--allow-host\", \"shop.example.com\""));
        assert!(d.contains("USER 10001"));
    }

    #[test]
    fn direct_はホストの口に直に出す_シークレットが無ければ_env_file_も無い() {
        let f = generate(&prog("direct", &[])).unwrap();
        let c = get(&f, "compose.yaml");
        assert!(c.contains("\"443:8080\""));
        assert!(!c.contains("env_file"));
        assert!(f.iter().all(|x| x.0 != "secrets.env.example"));
    }
}
