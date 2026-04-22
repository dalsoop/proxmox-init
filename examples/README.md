# 실전 예시

| 예시 | 설명 | 사용 도메인 |
|------|------|-------------|
| [formbricks.md](formbricks.md) | Formbricks 설문조사 + Traefik + CF DNS | lxc, traefik, cloudflare |
| [deploy-database.md](deploy-database.md) | PostgreSQL + Redis 한 줄 배포 | deploy, lxc, monitor, backup |
| [deploy-svn.md](deploy-svn.md) | 게임 협업용 SVN LXC + service + Cloudflare | deploy, lxc, service, cloudflare |
| [monitor-prometheus.md](monitor-prometheus.md) | monitor JSON → node_exporter textfile | monitor (--json) |

## 레시피 (deploy 전용)

| 레시피 | 서비스 |
|--------|--------|
| [recipes/nginx.toml](recipes/nginx.toml) | Nginx 정적 호스팅 |
| [recipes/postgres.toml](recipes/postgres.toml) | PostgreSQL 16 + 외부 접속 + 기본 DB/유저 |
| [recipes/redis.toml](recipes/redis.toml) | Redis 7 + requirepass + 외부 접속 |
| [recipes/uptime-kuma.toml](recipes/uptime-kuma.toml) | Uptime Kuma — 모니터링 (Docker, :3001) |
| [recipes/formbricks.toml](recipes/formbricks.toml) | Formbricks — 설문 플랫폼 (Docker Compose, :3000) |
| [recipes/matterbridge.toml](recipes/matterbridge.toml) | Matterbridge — 채팅 플랫폼 브릿지 (systemd) |
| [recipes/infisical.toml](recipes/infisical.toml) | Infisical — 오픈소스 시크릿 관리 (Docker Compose, :8080) |
| [recipes/ministack.toml](recipes/ministack.toml) | MiniStack — 로컬 AWS 에뮬레이터 (Docker, :4566) |
| [recipes/svn.toml](recipes/svn.toml) | 게임 협업용 Apache Subversion 서버 |

각 예시는 **Proxmox 빈 호스트 + prelik v1.5+** 를 전제로 합니다.

## 새 예시/레시피 제안

`examples/` 또는 `examples/recipes/` 에 파일 추가 + PR. 구조:

1. **사전 조건** — 어떤 도메인이 설치돼야 하나
2. **prelik 커맨드 순서** — 복붙해서 실행 가능한 형태
3. **검증 방법** — 동작 확인 한 줄
4. **정리** — 리소스 회수 (lxc delete 등)

## SVN 운영 메모

`shell-tools/bin/pxi-svn`를 설치한 환경이라면 다음 운영 명령을 쓸 수 있다.

```bash
pxi run svn repo-create --vmid 50123 --name prototype
pxi run svn repo-list --vmid 50123
pxi run svn user-list --vmid 50123
pxi run svn password --vmid 50123 --user admin
```
