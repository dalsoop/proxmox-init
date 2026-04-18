//! pxi-elk — ELK 스택 (Elasticsearch + Kibana + Logstash) 관리.

use clap::{Parser, Subcommand};
use pxi_core::common;

#[derive(Parser)]
#[command(name = "pxi-elk", about = "ELK 스택 관리 (ES + Kibana + Logstash)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

const ELK_IP: &str = "10.0.50.190"; // LINT_ALLOW: ELK LXC
const ELK_VMID: &str = "50190";

#[derive(Subcommand)]
enum Cmd {
    /// ELK 상태 확인 (ES + Kibana + Logstash)
    Status,
    /// Kibana 웹 UI URL 표시
    Open,
    /// 특정 LXC에서 ELK로 로그 전달 설정 (rsyslog)
    Connect {
        /// LXC VMID
        vmid: String,
    },
    /// LXC의 ELK 로그 전달 해제
    Disconnect {
        /// LXC VMID
        vmid: String,
    },
    /// 최근 로그 검색 (Elasticsearch query_string)
    Search {
        /// 검색 쿼리 (예: "level:error", "medusa AND path:/store")
        query: String,
        /// 최근 N분 (기본 30)
        #[arg(long, default_value = "30")]
        minutes: u32,
        /// 결과 수 (기본 20)
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// 인덱스 목록 + 크기
    Indices,
    /// 오래된 인덱스 정리 (기본 30일)
    Cleanup {
        /// 보존 일수
        #[arg(long, default_value = "30")]
        days: u32,
    },
    /// ES + Kibana + Logstash 재시작
    Restart,
    /// LXC에 ELK 설치 (Elasticsearch + Kibana + Logstash)
    Install {
        /// 대상 LXC VMID (기본: 50190)
        #[arg(long, default_value = "50190")]
        vmid: String,
    },
}

fn es_url() -> String {
    format!("http://{}:9200", ELK_IP)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let es = es_url();
    let kibana = "https://elk.50.internal.kr";

    match cli.cmd {
        Cmd::Status => {
            println!("=== Elasticsearch ===");
            common::run("curl", &["-sS", &format!("{}/_cluster/health?pretty", es)]);
            println!("\n=== Kibana ===");
            common::run("curl", &["-sS", "-o", "/dev/null", "-w", "HTTP %{http_code}\n",
                                  &format!("{}/api/status", kibana)]);
            println!("\n=== Logstash ===");
            common::run("pct", &["exec", ELK_VMID, "--", "systemctl", "is-active", "logstash"]);
        }
        Cmd::Open => {
            println!("Kibana: {}", kibana);
        }
        Cmd::Connect { vmid } => {
            let script = format!(
                "echo '*.* @@{}:5514' > /etc/rsyslog.d/90-elk.conf && \
                 apt-get install -y rsyslog 2>/dev/null; \
                 systemctl enable --now rsyslog && systemctl restart rsyslog && \
                 logger -t pxi-elk 'ELK connected from LXC {}'",
                ELK_IP, vmid
            );
            common::run("pct", &["exec", &vmid, "--", "bash", "-c", &script]);
            println!("✓ LXC {} → ELK ({}:5514) 연결됨", vmid, ELK_IP);
        }
        Cmd::Disconnect { vmid } => {
            common::run("pct", &["exec", &vmid, "--", "bash", "-c",
                        "rm -f /etc/rsyslog.d/90-elk.conf && systemctl restart rsyslog"]);
            println!("✓ LXC {} ELK 연결 해제됨", vmid);
        }
        Cmd::Search { query, minutes, limit } => {
            let body = format!(
                r#"{{"size":{},"sort":[{{"@timestamp":"desc"}}],"query":{{"bool":{{"must":[{{"query_string":{{"query":"{}"}}}},{{"range":{{"@timestamp":{{"gte":"now-{}m"}}}}}}]}}}}}}"#,
                limit, query.replace('"', "\\\""), minutes
            );
            let script = format!(
                "curl -sS '{}/syslog-*/_search' -H 'Content-Type: application/json' -d '{}' | \
                 python3 -c \"import json,sys; d=json.load(sys.stdin); \
                 [print(h['_source'].get('@timestamp','?')[:19], '|', \
                        h['_source'].get('log_level', h['_source'].get('medusa',{{}}).get('level','?'))[:5], '|', \
                        h['_source'].get('message','')[:120]) \
                  for h in d.get('hits',{{}}).get('hits',[])]\"",
                es, body
            );
            common::run("bash", &["-c", &script]);
        }
        Cmd::Indices => {
            common::run("curl", &["-sS", &format!("{}/_cat/indices?v&h=index,docs.count,store.size&s=index", es)]);
        }
        Cmd::Cleanup { days } => {
            println!("{}일 이전 syslog-* 인덱스 삭제 중...", days);
            let script = format!(
                "CUTOFF=$(date -u -d '{d} days ago' +syslog-%Y.%m.%d 2>/dev/null || \
                          date -u -v-{d}d +syslog-%Y.%m.%d); \
                 for idx in $(curl -sS '{es}/_cat/indices/syslog-*?h=index' | sort); do \
                   [[ \"$idx\" < \"$CUTOFF\" ]] && curl -sS -X DELETE '{es}/'$idx && echo \" deleted: $idx\"; \
                 done; echo \"cutoff: $CUTOFF\"",
                d = days, es = es
            );
            common::run("bash", &["-c", &script]);
        }
        Cmd::Restart => {
            common::run("pct", &["exec", ELK_VMID, "--", "bash", "-c",
                        "systemctl restart elasticsearch kibana logstash && \
                         sleep 5 && systemctl is-active elasticsearch kibana logstash"]);
        }
        Cmd::Install { vmid } => {
            println!("ELK 설치는 control-plane 레시피를 사용하세요:");
            println!("  cat /root/control-plane/services/elk.toml");
            println!("  대상 LXC: {}", vmid);
        }
    }
    Ok(())
}
