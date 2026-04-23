//! pxi-domain-lints — pxi 도메인 구조 규약 커스텀 lint 모음.
//!
//! 검사 항목:
//!   - pxi_domain_cli_name: 도메인 바이너리 Cli 구조체에 `#[command(name = "pxi-<domain>")]` 속성 필수
//!
//! 활성화: workspace Cargo.toml 에 [workspace.metadata.dylint] libraries 추가 후
//!         `cargo +nightly dylint --all` 실행.

#![feature(rustc_private)]
extern crate rustc_ast;
extern crate rustc_hir;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use dylint_linting::declare_late_lint;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};

declare_late_lint! {
    /// 도메인 바이너리의 Cli 구조체가 `#[command(name = "pxi-<domain>")]` 속성을
    /// 보유하는지 검사. 도메인 이름은 crate 이름에서 `pxi-` 접두어 제거로 추출.
    pub PXI_DOMAIN_CLI_NAME,
    Warn,
    "pxi 도메인 Cli 구조체에 #[command(name = \"pxi-<domain>\")] 속성 필수"
}

impl<'tcx> LateLintPass<'tcx> for PxiDomainCliName {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // Cli 구조체만 대상
        if !matches!(item.kind, ItemKind::Struct(..)) {
            return;
        }
        if item.ident.name.as_str() != "Cli" {
            return;
        }

        let crate_name = cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string();
        // pxi-<domain> 형식의 크레이트에서만 검사
        let Some(domain) = crate_name.strip_prefix("pxi-") else {
            return;
        };
        let expected_attr = format!("pxi-{domain}");

        // #[command(name = "...")] 속성 검색
        let has_correct_name = item.attrs.iter().any(|attr| {
            attr.path_matches(&rustc_ast::path!(command))
                && attr
                    .meta_item_list()
                    .unwrap_or_default()
                    .iter()
                    .any(|meta| {
                        meta.name_value().map_or(false, |(name, val)| {
                            name.as_str() == "name"
                                && val
                                    .value_str()
                                    .map_or(false, |s| s.as_str() == expected_attr)
                        })
                    })
        });

        if !has_correct_name {
            span_lint_and_help(
                cx,
                PXI_DOMAIN_CLI_NAME,
                item.span,
                &format!("Cli 구조체에 #[command(name = \"{expected_attr}\")] 속성이 없음"),
                None,
                &format!(
                    "#[derive(Parser)]\n#[command(name = \"{expected_attr}\")]\nstruct Cli {{ ... }}"
                ),
            );
        }
    }
}

dylint_linting::impl_late_lint_pass!(PxiDomainCliName, (PXI_DOMAIN_CLI_NAME,));
