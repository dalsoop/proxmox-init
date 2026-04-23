{
  schema_version = 1,

  blocked_paths = [
    "target/**",
    ".pxi/**",
    "**/*.deb",
    "**/*.tar.gz",
    "**/*.tar.xz",
    "**/*.zst",
  ],

  hardcoded_rules = [
    {
      name = "chrome-builder-network",
      description = "chrome-browser-dev 네트워크 값은 지정된 관리 파일에만 둔다",
      regex = "\\b10\\.0\\.50\\.(?:1|63|100|220)(?:/16)?\\b",
      allowed_globs = [
        "README.md",
        "crates/domains/chrome-browser-dev/src/main.rs",
        "crates/domains/chrome-browser-dev/scripts/*.sh",
        "ncl/policies/*.ncl",
        "examples/**/*.toml",
      ],
    },
    {
      name = "chrome-builder-paths",
      description = "builder workspace 경로는 지정된 관리 파일에만 둔다",
      regex = "/home/builder/workspace(?:/[A-Za-z0-9._/-]+)?",
      allowed_globs = [
        "README.md",
        "crates/domains/chrome-browser-dev/src/main.rs",
        "crates/domains/chrome-browser-dev/scripts/*.sh",
        "ncl/policies/*.ncl",
      ],
    },
    {
      name = "chrome-builder-temp-paths",
      description = "임시 실행 경로는 chrome-browser-dev 관리 파일에만 둔다",
      regex = "/tmp/helium-(?:x11|run)-[A-Za-z0-9._-]+",
      allowed_globs = [
        "crates/domains/chrome-browser-dev/src/main.rs",
        "crates/domains/chrome-browser-dev/scripts/*.sh",
        "ncl/policies/*.ncl",
      ],
    },
    {
      name = "chromedriver-port",
      description = "9515 포트 상수는 chrome-browser-dev 관리 파일에만 둔다",
      regex = "\\b9515\\b",
      allowed_globs = [
        "README.md",
        "crates/domains/chrome-browser-dev/src/main.rs",
        "crates/domains/chrome-browser-dev/scripts/*.sh",
        "ncl/policies/*.ncl",
      ],
    },
    {
      name = "gitlab-ssh-ip-remote",
      description = "GitLab SSH IP remote는 관리 파일에만 둔다",
      regex = "git@10\\.0\\.50\\.63:root/[A-Za-z0-9._-]+\\.git",
      allowed_globs = [
        "README.md",
        "crates/domains/chrome-browser-dev/src/main.rs",
        "ncl/policies/*.ncl",
      ],
    },
  ],

  companion_rules = [
    {
      name = "domain-index-sync",
      trigger_globs = ["crates/domains/*/domain.ncl"],
      requires_any_globs = ["ncl/domains.ncl"],
      message = "도메인 메타 변경 시 ncl/domains.ncl 인덱스도 함께 확인해야 한다",
    },
    {
      name = "chrome-browser-dev-doc-sync",
      trigger_globs = [
        "crates/domains/chrome-browser-dev/src/main.rs",
        "crates/domains/chrome-browser-dev/scripts/*.sh",
      ],
      requires_any_globs = [
        "README.md",
        "crates/domains/chrome-browser-dev/domain.ncl",
      ],
      message = "chrome-browser-dev 인터페이스/행동 변경 시 README 또는 domain.ncl도 함께 갱신해야 한다",
    },
  ],
}
