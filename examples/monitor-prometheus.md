# monitor JSON → Prometheus textfile collector

`prelik-monitor --json all`을 systemd timer로 1분마다 실행해서
node_exporter의 `--collector.textfile.directory`로 노출하는 예시.

## 사전 조건

- Proxmox 호스트에 prelik-init 설치
- node_exporter 설치 + textfile collector 활성화 (`/var/lib/node_exporter/textfile_collector`)
- `jq` (apt install jq)

## 1. 변환 스크립트

`/usr/local/bin/prelik-monitor-textfile.sh`:

```bash
#!/bin/bash
set -euo pipefail
OUT=/var/lib/node_exporter/textfile_collector/prelik.prom
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT

DATA=$(prelik run monitor --json all)

{
    echo "# HELP prelik_host_mem_used_pct 호스트 메모리 사용률"
    echo "# TYPE prelik_host_mem_used_pct gauge"
    echo "prelik_host_mem_used_pct $(echo "$DATA" | jq '.host.mem_used_pct')"

    echo "# HELP prelik_host_load1 호스트 1분 load average"
    echo "# TYPE prelik_host_load1 gauge"
    echo "prelik_host_load1 $(echo "$DATA" | jq -r '.host.load_avg[0]')"

    echo "# HELP prelik_lxc_running 실행 중 LXC 수"
    echo "# TYPE prelik_lxc_running gauge"
    LXC_N=$(echo "$DATA" | jq '.lxc | length')
    echo "prelik_lxc_running $LXC_N"

    echo "# HELP prelik_vm_running 실행 중 VM 수"
    echo "# TYPE prelik_vm_running gauge"
    VM_N=$(echo "$DATA" | jq '.vm | length')
    echo "prelik_vm_running $VM_N"

    # 디스크 사용률 (마운트별)
    echo "# HELP prelik_disk_used_pct 디스크 사용률 (%)"
    echo "# TYPE prelik_disk_used_pct gauge"
    echo "$DATA" | jq -r '.host.disks[] | "prelik_disk_used_pct{mount=\"\(.mount)\"} \(.use_pct | rtrimstr("%"))"'
} > "$TMP"

mv -f "$TMP" "$OUT"
```

```bash
chmod +x /usr/local/bin/prelik-monitor-textfile.sh
```

## 2. systemd unit + timer

`/etc/systemd/system/prelik-monitor-textfile.service`:
```ini
[Unit]
Description=prelik monitor → node_exporter textfile

[Service]
Type=oneshot
ExecStart=/usr/local/bin/prelik-monitor-textfile.sh
```

`/etc/systemd/system/prelik-monitor-textfile.timer`:
```ini
[Unit]
Description=prelik monitor 1분 주기

[Timer]
OnBootSec=30s
OnUnitActiveSec=60s
Unit=prelik-monitor-textfile.service

[Install]
WantedBy=timers.target
```

```bash
systemctl daemon-reload
systemctl enable --now prelik-monitor-textfile.timer
```

## 3. 검증

```bash
cat /var/lib/node_exporter/textfile_collector/prelik.prom
# Prometheus 쿼리 예:
#   prelik_host_mem_used_pct
#   prelik_disk_used_pct{mount="/"}
#   prelik_lxc_running
```

## fail-fast 안전성

`monitor --json`은 `pct`/`qm` 누락 시 EXIT 1 → systemd가
`failed` 상태 표시 → `prelik.prom`이 stale로 남지 않음 (이전 mv된 파일 유지).
이전 데이터가 stale을 정확히 반영하길 원하면 timer 앞에:

```ini
ExecStartPre=-/bin/rm -f /var/lib/node_exporter/textfile_collector/prelik.prom
```
