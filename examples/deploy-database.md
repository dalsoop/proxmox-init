# PostgreSQL + Redis 한 줄 배포

`prelik-deploy` 레시피로 데이터베이스 LXC 두 개를 띄운다.

## 사전 조건

- Proxmox 호스트 + prelik 설치 (`curl install.prelik.com | bash`)
- `prelik install bootstrap lxc deploy`

## 1. PostgreSQL LXC

```bash
DB_PASSWORD=s3cure prelik run deploy service \
  --recipe examples/recipes/postgres.toml \
  --vmid 200 \
  --hostname pg-main \
  --ip 10.0.50.200
```

기본값: `DB_NAME=app`, `DB_USER=app`. 환경변수로 오버라이드 가능.

## 2. Redis LXC

```bash
REDIS_PASSWORD=r3dis prelik run deploy service \
  --recipe examples/recipes/redis.toml \
  --vmid 201 \
  --hostname redis-main \
  --ip 10.0.50.201
```

## 3. 검증

```bash
prelik run lxc list
prelik run monitor --json lxc | jq '.[] | select(.vmid=="200" or .vmid=="201")'

# 호스트에서 직접 접속
psql -h 10.0.50.200 -U app -d app   # PostgreSQL
redis-cli -h 10.0.50.201 -a r3dis   # Redis
```

## 4. 백업 등록

```bash
# 매일 03:00 vzdump (zstd 압축, 보존 7일)
prelik run backup schedule-add --vmid 200 --hour 3 --keep 7
prelik run backup schedule-add --vmid 201 --hour 3 --keep 7
prelik run backup list
```

## 정리

```bash
prelik run lxc delete 200 --force
prelik run lxc delete 201 --force
```

## 레시피 작성법

[examples/recipes/](recipes/) 의 `*.toml` 참조. 핵심 섹션:
- `[service]` — name/description
- `[lxc]` — cores/memory/disk (deploy의 LXC 옵션이 이걸 덮어쓸 수 있음)
- `[install]` — packages 배열 + `[[install.steps]]` 배열 (각 step `name`/`run`)

각 `run`은 LXC 안에서 bash로 실행. 환경변수 그대로 통과.
