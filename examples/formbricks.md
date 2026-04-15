# 예시: Formbricks 설문조사 플랫폼 배포

Proxmox 호스트에서 빈 상태로 시작해서 `https://form.yourdomain.com` 에 설문조사 서비스가 떠있게 만드는 과정.

## 사전 조건

- Proxmox VE 호스트 + root SSH 접근
- Cloudflare 계정 (DNS 관리 중인 도메인)
- 공인 IP 또는 NAT 포트포워딩

## 1. prelik 설치 + 초기 세팅

```bash
curl -fsSL https://install.prelik.com | bash
prelik init
# Cloudflare/SMTP/Network 입력
```

## 2. 필요한 도메인 설치

```bash
prelik install bootstrap   # 의존성
prelik install lxc         # LXC 매니저
prelik install traefik     # 리버스 프록시
prelik install cloudflare  # DNS
```

## 3. LXC 생성

```bash
prelik run lxc create \
  --vmid 200 \
  --hostname formbricks \
  --ip 10.0.50.200
```

## 4. Formbricks Docker Compose 배포

```bash
# LXC 안에서 Docker + Formbricks 설치
pct exec 200 -- bash -c '
  apt-get update && apt-get install -y docker-compose-v2
  mkdir -p /opt/formbricks && cd /opt/formbricks
  curl -fsSL https://raw.githubusercontent.com/formbricks/formbricks/main/docker/docker-compose.yml -o docker-compose.yml
  docker compose up -d
'
```

## 5. Traefik 라우트 + DNS

```bash
# Traefik 재생성 (CF 크리덴셜 자동 주입)
prelik run traefik recreate --vmid 100

# 라우트 추가
prelik run traefik route-add \
  --vmid 100 \
  --name formbricks \
  --domain form.yourdomain.com \
  --backend http://10.0.50.200:3000 \
  --use-cf

# CF DNS A 레코드
prelik run cloudflare dns-add \
  --domain yourdomain.com \
  --type A \
  --name form \
  --content YOUR_PUBLIC_IP \
  --audience kr
```

## 6. 확인

```bash
curl -sI https://form.yourdomain.com/
# HTTP/2 200
```

## 응용: 이메일 수신 아카이브 (Mailpit + CF Email Worker)

```bash
prelik install mail
prelik run lxc create --vmid 124 --hostname mailpit --ip 10.0.50.124
prelik run mail install-mailpit --vmid 124
prelik run cloudflare email-worker-attach-all --worker my-mail-archive
```

`*@yourdomain.com` → Cloudflare → Worker → Mailpit SQLite 저장.
`https://mail.yourdomain.com` 에서 전체 메일 조회.
