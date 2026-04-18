# 호환성 매트릭스

각 도메인이 어떤 환경에서 동작하는지 표입니다. **prelik 자체는 Debian/Ubuntu x86_64/aarch64**를 지원하며, 일부 도메인은 Proxmox VE 호스트에서만 의미 있습니다.

## 환경 레전드

- **PVE**: Proxmox VE 호스트 (pct/qm/pvesh/vzdump 있는 환경)
- **Debian**: 순수 Debian/Ubuntu (Proxmox 아닌)
- **root**: root 권한 필수
- **user**: 일반 사용자로도 동작 (일부 기능 제한)

## 도메인별

| 도메인 | PVE | Debian | root | user | 비고 |
|---|---|---|---|---|---|
| **bootstrap** | ✓ | ✓ | ✓ | ✗ | apt install 필요 |
| **host** | ✓ | ✓ | ✓ | △ | status/monitor는 user OK, smb-open/ssh-keygen은 root |
| **account** | ✓ | ✓ | ✓ | ✗ | useradd/userdel |
| **nas** | ✓ | ✓ | ✓ | ✗ | mount/fstab/cifs-credentials |
| **workspace** | ✓ | ✓ | ✓ | ✓ | tmux/shell alias는 user 가능 |
| **lxc** | ✓ | ✗ | ✓ | ✗ | pct 필요 |
| **vm** | ✓ | ✗ | ✓ | ✗ | qm 필요 |
| **backup** | ✓ | ✗ | ✓ | ✗ | vzdump 필요 |
| **iso** | ✓ | ✗ | ✓ | ✗ | pvesm 필요 |
| **deploy** | ✓ | ✗ | ✓ | ✗ | pxi-lxc 의존 |
| **monitor** | ✓ | △ | ✓ | ✓ | host 커맨드는 어디서나, lxc/vm은 PVE만 |
| **net** | ✓ | ✓ | ✓ | ✓ | ip/ping/getent — 어디서나 |
| **node** | ✓ | ✗ | ✓ | ✗ | pvesh + ssh 필요 |
| **recovery** | ✓ | ✗ | ✓ | ✗ | /etc/pve 접근 필요 |
| **traefik** | ✓ | ✗ | ✓ | ✗ | Traefik LXC 관리 |
| **cloudflare** | ✓ | ✓ | ✓ | ✓ | curl + CF API key. PVE 불필요 |
| **mail** | ✓ | ✓ | ✓ | ✗ | Postfix 수정 |
| **connect** | ✓ | ✓ | ✓ | ✓ | dotenvx만 필요 |
| **telegram** | ✓ | ✓ | ✓ | ✓ | curl + Telegram API |
| **ai** | ✓ | ✓ | ✓ | ✗ | npm -g 필요 |
| **comfyui** | ✓ | ✗ | ✓ | ✗ | GPU passthrough + LXC |

## 아키텍처

| 아키텍처 | 빌드 | 실검증 |
|---|---|---|
| x86_64 | ✓ (CI) | ✓ (Proxmox VE 호스트) |
| aarch64 | ✓ (CI cross-build) | ✗ (미검증) |

## doctor로 확인

각 도메인의 `doctor` 명령이 현재 환경에서 필요한 도구가 있는지 점검합니다:

```bash
prelik run <domain> doctor
```

모든 doctor는 누락 도구를 보고만 하고 exit 0으로 종료합니다 (CI/스크립트 안전).
