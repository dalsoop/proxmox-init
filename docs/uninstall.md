# prelik 제거 가이드

prelik은 단순한 CLI가 아니라 시스템 곳곳을 변경합니다. **`prelik uninstall`은 prelik 바이너리만 지웁니다.** 시스템에 남는 변경을 어떻게 정리할지 이 문서에 상세히 적혀 있습니다.

> ⚠️ **읽지 않고 실행 금지.** 어떤 단계는 데이터 유실로 이어질 수 있습니다 (LXC 디스크, 시크릿, 메일 큐 등). 매 단계마다 백업하거나 dry-run 먼저 돌려보세요.

---

## TL;DR — 시나리오별

| 상황 | 명령 |
|---|---|
| 그냥 prelik만 떼고 시스템은 그대로 | `prelik uninstall` (dry-run) → 확인 후 `--confirm` |
| 설정/스냅샷 까지 모두 삭제 | `prelik uninstall --confirm --purge` |
| LXC/VM/NAS/메일/CF까지 완전 청소 | 이 문서 §2~§9를 순서대로 |

---

## 1. `prelik uninstall`이 실제로 하는 일

- `/usr/local/bin/prelik` + `prelik-*` 도메인 바이너리 19개 + `.prelik.version` 마커 삭제
- `~/.local/bin` 도 같은 패턴으로 처리
- **`--purge`** 추가 시:
  - `~/.config/prelik` (사용자 config)
  - `/etc/prelik` (시스템 config)
  - `~/.local/share/prelik` 또는 `/var/lib/prelik` (도메인 캐시 + recovery snapshots + audit log)

**기본적으로는 다음을 절대 건드리지 않습니다:**

- 운영 중인 LXC/VM (`pct/qm` 리소스)
- `/etc/fstab` 안의 NAS 마운트 라인
- `/etc/cifs-credentials/*` (SMB 자격증명 파일)
- `/etc/postfix/main.cf` + sasl_passwd + sender_canonical
- traefik 컨테이너, 라우트 JSON, TLS 인증서
- Cloudflare에 등록한 DNS 레코드, Worker, Pages 프로젝트
- dotenvx로 암호화된 `.env.vault` (다른 위치라면 `--purge`도 보존)
- systemd timers/services (예: `cluster-files-sync.timer`)

이는 **고의**입니다. 사용자가 명시적으로 결정해야 안전한 항목들이라서.

---

## 2. LXC/VM 정리 (가장 위험 — 데이터 유실)

prelik으로 만든 LXC/VM은 그대로 살아 있습니다. **디스크 데이터까지 전부 사라집니다.**

```bash
# 1) 살아있는 prelik 출처 LXC 확인
prelik run lxc list                 # 또는: pct list

# 2) 백업 (필수 권장)
prelik run backup now <VMID>        # vzdump 스냅샷
# 또는: vzdump <VMID> --storage local --compress zstd --mode snapshot

# 3) 정지 + 삭제
prelik run lxc stop <VMID>
prelik run lxc delete <VMID> --force   # 디스크까지 삭제

# VM은 prelik run vm delete --force <VMID>
```

**경고:** `--force`는 물리 디스크와 스냅샷까지 영구 삭제. 백업 전에 절대 실행 금지.

---

## 3. NAS 마운트 (fstab + credentials)

`prelik run nas mount`로 추가한 SMB/NFS 마운트가 `/etc/fstab`에 남아 있습니다.

```bash
# 1) 현재 마운트 확인
findmnt --target /mnt | grep -E "cifs|nfs"

# 2) prelik이 추가한 라인 식별 (보통 # prelik-managed 주석 마커 옆)
sudo grep -n "prelik\|cifs-credentials" /etc/fstab

# 3) 언마운트
sudo umount /mnt/<your-mount>

# 4) fstab에서 해당 라인 제거
sudo nano /etc/fstab    # 또는 sed -i '/PATTERN/d'

# 5) cifs-credentials 파일 제거 (SMB 비밀번호 평문)
#    ⚠️ 주의 1: prelik nas는 파일명을 "{host}_{share}" 로 만들되,
#      비영숫자 문자(점/슬래시 등)를 모두 '_'로 치환합니다.
#      예) //nas.local/data → 'nas_local_data' (점도 _로 치환됨)
#          //10.0.0.5/media → '10_0_0_5_media'
#    ⚠️ 주의 2: provenance 마커가 없어 다른 CIFS mount의 자격증명과 구분 불가.
#      ls로 확인 후 prelik이 추가한 마운트에 해당하는 파일만 골라 삭제.

# 디렉토리 내 파일 확인
sudo ls -la /etc/cifs-credentials/

# 본인이 prelik nas mount로 추가한 host/share에 대응하는 safe_name만 삭제 (예시)
sudo rm -f /etc/cifs-credentials/nas_local_data

# 디렉토리가 비었을 때만 (다른 mount의 cred가 남아 있으면 보존)
sudo rmdir /etc/cifs-credentials 2>/dev/null || true
```

**검증:**
```bash
sudo mount -a   # fstab 변경 후 잘못된 항목 없는지 확인
```

---

## 4. Postfix relay (메일 발송)

`prelik run mail postfix-relay`가 `main.cf`를 수정하고 `prelik-backup-<ns-ts>/` 백업 디렉토리를 만들었습니다.

```bash
# 1) 백업 디렉토리 확인
sudo ls /etc/postfix/prelik-backup-*

# 2) 가장 최근(또는 prelik 이전) 백업으로 복원
LATEST=$(ls -td /etc/postfix/prelik-backup-* | tail -1)
sudo cp "$LATEST/main.cf" /etc/postfix/main.cf
sudo cp "$LATEST/sasl_passwd" /etc/postfix/sasl_passwd 2>/dev/null
sudo cp "$LATEST/sender_canonical" /etc/postfix/sender_canonical 2>/dev/null
sudo postmap /etc/postfix/sasl_passwd
sudo postmap /etc/postfix/sender_canonical
sudo systemctl reload postfix

# 3) 백업 디렉토리들 정리
sudo rm -rf /etc/postfix/prelik-backup-*
```

**경고:** 백업 디렉토리가 여러 개면 가장 첫 백업을 써야 prelik 도입 이전 상태가 됩니다. timestamp 가장 작은 것.

---

## 5. Traefik (리버스 프록시)

`prelik run traefik recreate`로 띄운 LXC + 라우트가 남아 있습니다.

```bash
# Traefik LXC가 별도면 §2 절차로 LXC 삭제
prelik run lxc list | grep traefik

# 라우트 JSON (보통 traefik LXC 안의 /etc/traefik/dynamic/)
# LXC 안에서: rm /etc/traefik/dynamic/*.yml
# TLS 인증서 (acme.json):
#   - LXC 안의 /etc/traefik/acme.json — Let's Encrypt 발급분, 백업 권장
```

Cloudflare 측의 Workers/Pages는 §6 참조.

---

## 6. Cloudflare (DNS / Email Worker / Pages)

prelik이 만든 Cloudflare 리소스는 자동으로 안 지웁니다.

```bash
# DNS 레코드 일람
prelik run cloudflare dns-list --domain example.com

# 개별 삭제
prelik run cloudflare dns-delete --domain example.com --name myapp --type A

# Email Worker
# Cloudflare 대시보드 → Email Routing → Routing Rules → 수동 disable
# 또는: wrangler delete <worker-name>

# Pages 프로젝트
# wrangler pages project delete <project-name>
```

⚠️ DNS 레코드 잘못 지우면 라이브 서비스 다운. 반드시 외부 의존성 확인 후.

---

## 7. dotenvx 시크릿

`prelik run connect set/encrypt`로 만든 `.env` + `.env.vault` + `.env.keys`가 프로젝트 디렉토리에 남아 있습니다.

```bash
# 보통 위치
ls -la /etc/prelik/.env*       # 시스템
ls -la ~/.config/prelik/.env*  # 사용자
ls -la <your-project>/.env*    # 각 프로젝트

# .env.keys는 매스터키 — 잃으면 .env.vault 영원히 복호화 불가
# 보존하려면 별도 안전한 곳에 옮긴 뒤 삭제
```

`prelik uninstall --purge`는 표준 위치만 지웁니다. 다른 곳의 .env.vault는 안 건드림.

---

## 8. systemd timers/services

`prelik install ai`, `prelik run ai openclaw-setup` 등이 systemd 유닛을 등록할 수 있습니다.

```bash
# prelik이 등록한 유닛 목록
sudo systemctl list-unit-files | grep -E "prelik|cluster-files|adversarial"

# 비활성화 + 삭제
sudo systemctl disable --now <unit-name>
sudo rm -f /etc/systemd/system/<unit-name>
sudo systemctl daemon-reload
```

---

## 9. dotenvx / nickel / rust 등 의존성 도구

`prelik install bootstrap`이 시스템 패키지를 깔았습니다. 다른 용도로도 쓸 수 있어 자동 제거 안 됩니다.

```bash
# 깐 것들
which dotenvx nickel cargo gh

# 개별 제거 (예시)
sudo apt remove --purge gh
cargo uninstall ...    # cargo로 깐 것들
sudo rm -f /usr/local/bin/dotenvx
```

---

## 10. 완료 확인

```bash
# 바이너리 흔적
which prelik prelik-lxc prelik-ai 2>&1 | grep -v "no.*in"

# config/state 흔적
ls -la ~/.config/prelik /etc/prelik /var/lib/prelik 2>&1 | grep -v "No such"

# systemd 흔적
sudo systemctl list-unit-files 2>/dev/null | grep prelik

# fstab/cifs/postfix 흔적
sudo grep prelik /etc/fstab
ls /etc/cifs-credentials/ 2>/dev/null
ls /etc/postfix/prelik-backup-* 2>/dev/null
```

전부 비어 있으면 완전 제거 완료.

---

## 11. 복구

`--purge` 했어도 복구 가능:
- LXC/VM은 vzdump 백업으로 복원 (`prelik run backup restore` 또는 `pct restore`)
- recovery snapshot으로 LXC config만 복원: `prelik run recovery list/restore`
- postfix는 `prelik-backup-*/` 디렉토리에서 main.cf 복원

`.env.vault` + `.env.keys` 둘 다 잃으면 복구 **불가능**. 시크릿 재발급밖에 없음.

---

## 보안 주의

- `--purge`는 audit log (`/var/lib/prelik/audit.log`)도 지웁니다. **컴플라이언스 환경**에선 먼저 외부에 백업하세요.
- recovery snapshot은 LXC config(메모리/네트워크 설정)를 평문 저장. NAS/SMB 자격증명이 컨테이너 환경변수로 들어 있다면 snapshot에도 포함됩니다.
- `cifs-credentials/*`는 0600 root:root지만 평문. 디스크 dispose 전에 반드시 `shred -u`.
