# SVN LXC 배포

게임용 버전관리 서버를 `pxi`의 기존 도메인 조합만으로 올린다.

원하면 얇은 래퍼인 `pxi-svn`으로 한 번에 실행할 수 있다.

```bash
pxi run svn install --vmid 50123 --hostname svn --domain svn.50.internal.kr
```

## 사전 조건

- Proxmox 호스트 + `pxi`
- `pxi install lxc deploy service cloudflare`
- Cloudflare에 `internal.kr` zone 존재
- Traefik/`service-sync`가 이미 쓰이는 환경

## 1. SVN LXC 생성 + 내부 초기화

```bash
pxi run deploy service \
  --recipe examples/recipes/svn.toml \
  --vmid 50123 \
  --hostname svn \
  --ip 10.0.50.123
```

배포가 끝나면 기본 checkout 주소가 LXC 안의 `/root/svn-bootstrap.txt`에 저장된다.  
비밀번호는 LXC 내부 `/root/.env`에 `dotenvx`로 암호화되고, 키 정본은 호스트 `/root/control-plane/.secrets/pxi-svn/<vmid>.env.keys`에 둔다.

확인:

```bash
pct exec 50123 -- cat /root/svn-bootstrap.txt
pxi run svn password --vmid 50123 --user admin
```

## 2. Traefik 서비스 등록

`service` 그룹은 `50.internal.kr` 기준으로 잡는다.

```bash
pxi run service add \
  --domain 50.internal.kr \
  --name svn \
  --host svn.50.internal.kr \
  --ip 10.0.50.123 \
  --port 80 \
  --vmid 50123
```

확인:

```bash
pxi run service info svn
pct exec 50100 -- cat /opt/traefik/dynamic/svn.yml
```

## 3. Cloudflare DNS 등록

Cloudflare zone은 `50.internal.kr`가 아니라 `internal.kr`이고, 레코드 이름은 `svn.50`이다.

```bash
pxi run cloudflare dns-upsert \
  --domain internal.kr \
  --type A \
  --name svn.50 \
  --content 10.0.50.123 \
  --proxied false
```

확인:

```bash
pxi run cloudflare dns-list --domain internal.kr
getent hosts svn.50.internal.kr
```

## 4. 검증

```bash
curl -I http://10.0.50.123/
svn ls --non-interactive \
  --username admin \
  --password "$(pxi run svn password --vmid 50123 --user admin)" \
  http://10.0.50.123/svn/game
```

도메인까지 붙었으면:

```bash
curl -I http://svn.50.internal.kr/
```

## 5. 운영 메모

- 저장소 기본 이름은 `game`
- 새 저장소 생성은 `pxi run svn repo-create --vmid 50123 --name <repo>`
- 초기 관리자 계정은 `admin`
- 저장소 목록 확인은 `pxi run svn repo-list --vmid 50123`
- 비밀번호 조회는 `pxi run svn password --vmid 50123 --user <admin|modeler|programmer>`
- 사용자 추가는 `pxi run svn user-add --vmid 50123 --username <user> --group <artists|programmers|admins|external> --password '<pw>'`
- 백업은 LXC 내부 `03:15` cron + 호스트의 `pxi run backup ...` 조합으로 가져간다

## 정리

```bash
pxi run service remove svn --force
pxi run lxc delete 50123 --force
pxi run cloudflare dns-delete --domain internal.kr --type A --name svn.50
```
