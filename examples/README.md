# 실전 예시

| 예시 | 설명 | 사용 도메인 |
|------|------|-------------|
| [formbricks.md](formbricks.md) | Formbricks 설문조사 + Traefik + CF DNS | lxc, traefik, cloudflare |
| [deploy-database.md](deploy-database.md) | PostgreSQL + Redis 한 줄 배포 | deploy, lxc, monitor, backup |
| [monitor-prometheus.md](monitor-prometheus.md) | monitor JSON → node_exporter textfile | monitor (--json) |

## 레시피 (deploy 전용)

| 레시피 | 서비스 |
|--------|--------|
| [recipes/nginx.toml](recipes/nginx.toml) | Nginx 정적 호스팅 |
| [recipes/postgres.toml](recipes/postgres.toml) | PostgreSQL 16 + 외부 접속 + 기본 DB/유저 |
| [recipes/redis.toml](recipes/redis.toml) | Redis 7 + requirepass + 외부 접속 |

각 예시는 **Proxmox 빈 호스트 + prelik v1.5+** 를 전제로 합니다.

## 새 예시/레시피 제안

`examples/` 또는 `examples/recipes/` 에 파일 추가 + PR. 구조:

1. **사전 조건** — 어떤 도메인이 설치돼야 하나
2. **prelik 커맨드 순서** — 복붙해서 실행 가능한 형태
3. **검증 방법** — 동작 확인 한 줄
4. **정리** — 리소스 회수 (lxc delete 등)
