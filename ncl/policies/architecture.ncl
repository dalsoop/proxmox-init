{
  schema_version = 1,

  scan_globs = [
    "crates/domains/*/src/**/*.rs",
    "crates/domains/*/scripts/*.sh",
  ],

  ignore_refs = {
    # self-reference는 자동 허용된다.
    # 추가 예외가 필요하면 여기에 domain -> [refs...] 로 넣는다.
  },
}
