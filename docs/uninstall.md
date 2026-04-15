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

## 3. NAS 마운트 (fstab + credentials) — 순서 엄수

`prelik run nas mount`는 SMB/CIFS와 NFS 마운트 둘 다 `/etc/fstab`에 영구 등록합니다. 각 경로가 다르니 둘 다 살펴야 합니다.

### 3.1 SMB/CIFS 정리 (credentials 파일 포함)

> ⚠️ **순서가 중요합니다.** fstab 라인을 먼저 지우면 어느 credential 파일이 prelik 것인지 역추적 불가능. 아래 단계는 "캡처 → 편집 → 삭제" 순서로 배치돼 있습니다.
>
> prelik의 CIFS 라인 시그니처: fstab 옵션 필드에 `credentials=/etc/cifs-credentials/…` 경로가 들어갑니다. 이 prefix 경로는 prelik 전용이라 필터로 충분히 정확합니다. 타 관리자도 동일한 디렉토리를 쓰면 아래 단계에서 공유될 수 있으니 `sudo ls /etc/cifs-credentials/`로 사전 확인을 권장합니다.

```bash
# 1) 현재 CIFS 마운트 확인
findmnt --target /mnt | awk '$NF ~ /cifs|smb/'

# 2) prelik SMB fstab 라인 식별 (credentials 경로 단독 필터 — 옵션 순서 무관)
sudo grep -nE 'credentials=/etc/cifs-credentials/' /etc/fstab
# 출력이 없으면 3.1은 건너뜀.

# 3) ★ 순서 중요 ★ — fstab 편집 전에 credentials 경로를 변수로 먼저 캡처
CRED_PATHS=$(sudo grep -E 'credentials=/etc/cifs-credentials/' /etc/fstab \
  | grep -oE 'credentials=[^, ]+' | cut -d= -f2- | sort -u)
echo "prelik 관리 credentials 파일:"
printf '  %s\n' $CRED_PATHS

# 4) 해당 라인의 mount point 추출 후 각각 umount
sudo grep -E 'credentials=/etc/cifs-credentials/' /etc/fstab | awk '{print $2}'
# 위 출력으로 나온 각 경로에 대해:
sudo umount /mnt/<your-prelik-mount>

# 5) fstab에서 prelik SMB 라인만 제거. sed -i.bak로 백업.
sudo sed -i.bak '/credentials=\/etc\/cifs-credentials\//d' /etc/fstab
# 의도하지 않은 삭제 확인:
sudo diff /etc/fstab.bak /etc/fstab

# 6) 캡처한 credentials 파일만 정확히 삭제
for p in $CRED_PATHS; do
  sudo rm -f "$p"
done

# 7) 디렉토리가 비었을 때만 rmdir (다른 credential 보존)
sudo rmdir /etc/cifs-credentials 2>/dev/null || true

# 8) 최종 검증
sudo mount -a
```

### 3.2 NFS 정리

NFS는 credentials 파일이 없고, prelik이 등록한 fstab 라인 형식은 다음과 같습니다:

```
<server>:<export>  <mount-point>  nfs  _netdev,nofail  0  0
```

이 필드 조합(type=`nfs` + 옵션=`_netdev,nofail`)이 prelik 시그니처입니다. 타 관리자가 NFS를 수동 등록했고 같은 옵션을 썼다면 구분되지 않으니, **삭제 전 반드시 라인을 사람이 확인**하세요.

```bash
# 1) prelik NFS 라인 식별 (type 필드가 3번째 컬럼, 옵션이 4번째)
sudo awk '$3 == "nfs" && $4 == "_netdev,nofail" { print NR": "$0 }' /etc/fstab

# 2) 각 라인의 mount point 확인 후 umount
sudo awk '$3 == "nfs" && $4 == "_netdev,nofail" { print $2 }' /etc/fstab
# 각 mount point에 대해:
sudo umount /mnt/<your-prelik-nfs-mount>

# 3) fstab에서 prelik NFS 라인만 제거 (awk 필터링 + root 소유 임시파일로 atomic replace)
#    /tmp는 다른 사용자가 symlink로 선점 가능 → root 소유 /etc에 임시파일 생성.
TMP=$(sudo mktemp /etc/fstab.new.XXXXXX)
sudo awk '!($3 == "nfs" && $4 == "_netdev,nofail")' /etc/fstab | sudo tee "$TMP" >/dev/null
sudo diff /etc/fstab "$TMP"   # 삭제 예정 라인 확인
sudo cp -p /etc/fstab /etc/fstab.bak
sudo chown root:root "$TMP" && sudo chmod 644 "$TMP"
sudo mv "$TMP" /etc/fstab

# 4) 최종 검증
sudo mount -a
```

**전반 안전 장치:**
- 캡처 → 편집 → 삭제 순서 (편집이 먼저 오면 추적 불가)
- fstab.bak 백업 + diff 검증
- `mount -a`로 잘못된 항목 없는지 최종 확인

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

## 9. dotenvx / nickel / rust 등 의존성 도구 (bootstrap)

`prelik install bootstrap`이 시스템 패키지/바이너리를 설치했습니다. 다른 용도로도 쓸 수 있어 자동 제거 안 합니다.

**먼저 manifest로 정확히 무엇이 깔렸는지 확인:**

```bash
prelik run bootstrap manifest             # 사람용 표
prelik run bootstrap manifest --json      # 자동화/스크립트용
prelik run bootstrap manifest --only gh   # 특정 도구만
```

각 도구가 깐 정확한 apt 패키지/바이너리 경로/추가 파일 + 제거 절차가 출력됩니다.

**예시 (gh):**
```
[gh]
  apt 패키지: gh
  바이너리:   /usr/bin/gh
  파일:       /etc/apt/sources.list.d/github-cli.list,
              /usr/share/keyrings/githubcli-archive-keyring.gpg,
              ~/.config/gh/
  제거 절차:
    sudo apt remove --purge gh
    sudo rm -f /etc/apt/sources.list.d/github-cli.list \
               /usr/share/keyrings/githubcli-archive-keyring.gpg
    rm -rf ~/.config/gh    # 인증 토큰 포함
```

manifest의 "제거 절차" 그대로 실행하면 됩니다. apt 패키지는 `apt autoremove`를 절대 즉시 실행하지 마세요 — `build-essential` 등이 다른 시스템 패키지의 의존이라 함께 사라질 수 있습니다.

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
