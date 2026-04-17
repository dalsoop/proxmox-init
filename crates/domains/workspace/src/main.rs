//! prelik-workspace — 개발자 작업 환경 초기화.
//! tmux conf/start + shell alias (BEGIN/END markers) + lxc-shell + lxc-tmux + nvim-markdown + status.

use clap::{Parser, Subcommand, ValueEnum};
use prelik_core::common;
use prelik_core::helpers;

use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "prelik-workspace", about = "작업 환경 (tmux + shell 도구 + LXC 환경)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// tmux 기본 설정 배포 (~/.tmux.conf + 플러그인)
    TmuxSetup,
    /// tmux 세션 시작 (계정별 윈도우)
    TmuxStart,
    /// shell alias + 편의 도구 설정 (BEGIN/END 마커 관리)
    ShellSetup,
    /// Markdown 전용 Neovim 프로필 설치 (mdnvim wrapper + Marksman + preview)
    NvimMarkdown {
        /// 프로필: minimal / autolist / markview
        #[arg(long, value_enum, default_value = "minimal")]
        profile: MarkdownProfile,
    },
    /// LXC 내부 셸 환경 설정 (도구 + alias + git author)
    LxcShell {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
    },
    /// LXC 내부 tmux 설정 (플러그인 + conf)
    LxcTmux {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
    },
    /// 전체 작업 환경 상태 확인
    Status,
    /// 의존 도구 점검
    Doctor,
}

#[derive(Clone, Debug, ValueEnum)]
enum MarkdownProfile {
    Minimal,
    Autolist,
    Markview,
}

impl MarkdownProfile {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Autolist => "autolist",
            Self::Markview => "markview",
        }
    }
}

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

const SHELL_TOOLS: &[&str] = &["fzf", "zoxide", "fd-find", "bat", "eza"];

const ALIAS_BEGIN: &str = "# BEGIN: prelik-shell-setup v1";
const ALIAS_END: &str = "# END: prelik-shell-setup v1";

const ALIAS_BLOCK: &str = r#"
# BEGIN: prelik-shell-setup v1
# (managed by prelik workspace shell-setup -- 수동 수정 금지)
alias ll='eza -alh'
alias ls='eza'
alias lt='eza --tree --level=2'
alias cat='batcat --paging=never'
alias gs='git status'
alias gd='git diff'
if command -v zoxide >/dev/null 2>&1; then
    eval "$(zoxide init bash)"
fi

# tmux 세션 숫자 스위치 (1~99 입력 시 해당 세션, 없으면 최근 tmux)
__prelik_tmux_jump() {
    local n="$1" target
    if [ -n "$TMUX" ]; then
        tmux switch-client -t ="$n" 2>/dev/null && return
        tmux switch-client -l 2>/dev/null && return
        target=$(tmux list-sessions -F '#{session_activity} #{session_name}' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
        [ -n "$target" ] && tmux switch-client -t ="$target"
    else
        tmux attach -t ="$n" 2>/dev/null && return
        target=$(tmux list-sessions -F '#{session_activity} #{session_name}' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
        if [ -n "$target" ]; then
            tmux attach -t ="$target"
        else
            tmux
        fi
    fi
}
for _n in $(seq 1 99); do
    alias $_n="__prelik_tmux_jump $_n"
done
unset _n
# END: prelik-shell-setup v1
"#;

const TMUX_PLUGINS: &[(&str, &str)] = &[
    ("tpm", "https://github.com/tmux-plugins/tpm"),
    ("tmux-resurrect", "https://github.com/tmux-plugins/tmux-resurrect"),
    ("tmux-continuum", "https://github.com/tmux-plugins/tmux-continuum"),
];

const TMUX_CONF: &str = "\
# prelik-workspace 자동 생성
set -g default-terminal \"tmux-256color\"
set -ga terminal-overrides \",*256col*:Tc\"
set -g mouse on
set -g history-limit 100000
set -g base-index 1
setw -g pane-base-index 1
set -g renumber-windows on
set -g status-interval 5
set -sg escape-time 0
bind | split-window -h -c \"#{pane_current_path}\"
bind - split-window -v -c \"#{pane_current_path}\"
bind c new-window -c \"#{pane_current_path}\"
bind r source-file ~/.tmux.conf \\; display \"Reloaded\"
bind h select-pane -L
bind j select-pane -D
bind k select-pane -U
bind l select-pane -R
set -g status-position bottom
set -g status-left \"[#S] \"
set -g status-right \"#H | %Y-%m-%d %H:%M\"

# Plugins
set -g @plugin 'tmux-plugins/tpm'
set -g @plugin 'tmux-plugins/tmux-resurrect'
set -g @plugin 'tmux-plugins/tmux-continuum'

set -g @continuum-restore 'on'
set -g @continuum-save-interval '15'
set -g @resurrect-capture-pane-contents 'on'

run '~/.tmux/plugins/tpm/tpm'
";

const TMUX_SESSION: &str = "prelik";

const MDNVIM_APPNAME: &str = "mdnvim";
const MDNVIM_WRAPPER: &str = "/usr/local/bin/mdnvim";
const MDNVIM_MARKER: &str = "-- prelik-workspace mdnvim";
const MDNVIM_PACKAGES: &[&str] = &[
    "neovim", "git", "curl", "ripgrep", "fd-find", "unzip", "build-essential",
];

const LXC_GITHUB_TOOLS: &[(&str, &str)] = &[
    ("lazygit", "jesseduffield/lazygit"),
    ("lazydocker", "jesseduffield/lazydocker"),
];

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::TmuxSetup => tmux_setup(),
        Cmd::TmuxStart => tmux_start(),
        Cmd::ShellSetup => shell_setup(),
        Cmd::NvimMarkdown { profile } => nvim_markdown_setup(&profile),
        Cmd::LxcShell { vmid } => lxc_shell_setup(&vmid),
        Cmd::LxcTmux { vmid } => lxc_tmux_setup(&vmid),
        Cmd::Status => { status(); Ok(()) }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// ---------------------------------------------------------------------------
// tmux setup (포트 from workspace/tmux.rs)
// ---------------------------------------------------------------------------

fn tmux_setup() -> anyhow::Result<()> {
    println!("=== tmux 설정 ===\n");
    if !common::has_cmd("tmux") {
        anyhow::bail!("tmux 미설치 -- sudo apt install tmux");
    }

    let home = home_dir()?;
    install_tmux_plugins(&home)?;

    let conf_path = home.join(".tmux.conf");
    if conf_path.exists() {
        let backup = home.join(format!(".tmux.conf.prelik-backup-{}",
            common::run("date", &["+%Y%m%d-%H%M%S"]).unwrap_or_default().trim()));
        fs::copy(&conf_path, &backup)?;
        println!("  백업: {}", backup.display());
    }
    fs::write(&conf_path, TMUX_CONF)?;
    println!("+ {} 배포 완료", conf_path.display());
    println!("  적용: tmux source-file ~/.tmux.conf");
    Ok(())
}

fn install_tmux_plugins(home: &Path) -> anyhow::Result<()> {
    let plugins_dir = home.join(".tmux/plugins");
    for (name, url) in TMUX_PLUGINS {
        let plugin_dir = plugins_dir.join(name);
        if plugin_dir.exists() {
            println!("[tmux] {name} 이미 설치됨");
            continue;
        }
        fs::create_dir_all(&plugins_dir)?;
        common::run("git", &["clone", url, &plugin_dir.display().to_string()])?;
        println!("[tmux] {name} 설치 완료");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// tmux start (포트 from workspace/tmux.rs start())
// ---------------------------------------------------------------------------

fn tmux_start() -> anyhow::Result<()> {
    println!("=== tmux 세션 시작 ===\n");
    let exists = common::run("tmux", &["has-session", "-t", TMUX_SESSION]).is_ok();
    if exists {
        println!("[tmux] 세션 '{TMUX_SESSION}' 이미 존재.\n  tmux attach -t {TMUX_SESSION}");
        return Ok(());
    }

    // 기본 세션 생성
    common::run_bash(&format!(
        "tmux new-session -d -s {TMUX_SESSION} -n main"
    ))?;
    println!("[tmux] 세션 '{TMUX_SESSION}' 생성 완료");
    println!("\n접속: tmux attach -t {TMUX_SESSION}");
    Ok(())
}

// ---------------------------------------------------------------------------
// shell setup (포트 from workspace/shell.rs)
// ---------------------------------------------------------------------------

fn shell_setup() -> anyhow::Result<()> {
    println!("=== 셸 환경 설정 ===\n");

    // 도구 설치
    install_shell_tools()?;

    // .bashrc alias 관리
    let home = home_dir()?;
    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        let existing = fs::read_to_string(&bashrc)?;
        let (next, action) = reconcile_bashrc(&existing);
        match action {
            ReconcileAction::Unchanged => println!("[shell] alias 최신 (skip)"),
            _ => {
                fs::write(&bashrc, &next)?;
                println!("[shell] {}", action.message());
            }
        }
    } else {
        // .bashrc가 없으면 alias 블록만 생성
        fs::write(&bashrc, ALIAS_BLOCK)?;
        println!("[shell] .bashrc 생성 + alias 추가");
    }

    // .bashrc.d 방식도 지원
    let rc_dir = home.join(".bashrc.d");
    fs::create_dir_all(&rc_dir)?;
    let rc_path = rc_dir.join("prelik.sh");
    let shell_rc = "# prelik-workspace shell extras\n\
        if command -v bat >/dev/null; then alias cat='bat --paging=never'; fi\n\
        if command -v eza >/dev/null; then alias ls='eza'; fi\n\
        alias g='git'\nalias t='tmux'\nalias ta='tmux attach -t'\nalias tl='tmux ls'\n";
    fs::write(&rc_path, shell_rc)?;

    // ~/.bashrc에 source 줄 추가 (없으면)
    let bashrc_content = fs::read_to_string(home.join(".bashrc")).unwrap_or_default();
    if !bashrc_content.contains("bashrc.d") {
        let source_line = "\n# prelik-workspace\nfor f in ~/.bashrc.d/*.sh; do [ -r \"$f\" ] && source \"$f\"; done\n";
        fs::write(home.join(".bashrc"), bashrc_content + source_line)?;
        println!("[shell] ~/.bashrc에 bashrc.d source 줄 추가");
    }

    println!("\n=== 셸 환경 설정 완료 ===");
    Ok(())
}

fn install_shell_tools() -> anyhow::Result<()> {
    let missing: Vec<&&str> = SHELL_TOOLS.iter().filter(|p| !pkg_installed(p)).collect();
    if missing.is_empty() {
        println!("[apt] 셸 도구 이미 설치됨");
        return Ok(());
    }
    let pkgs = missing.iter().map(|p| **p).collect::<Vec<_>>().join(" ");
    println!("[apt] 셸 도구 설치 중: {pkgs}");
    common::run_bash(&format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkgs}"))?;
    Ok(())
}

fn pkg_installed(pkg: &str) -> bool {
    common::run_bash(&format!("dpkg -s {pkg} 2>/dev/null | grep -q 'Status.*installed'")).is_ok()
}

// ---------------------------------------------------------------------------
// alias reconcile (BEGIN/END marker management, 포트 from workspace/shell.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum ReconcileAction {
    Unchanged,
    Installed,
    Updated,
    Migrated,
}

impl ReconcileAction {
    fn message(&self) -> &'static str {
        match self {
            ReconcileAction::Unchanged => "alias 최신",
            ReconcileAction::Installed => "alias 추가 완료",
            ReconcileAction::Updated => "alias 블록 갱신",
            ReconcileAction::Migrated => "레거시 블록 제거 + alias 블록 갱신",
        }
    }
}

fn reconcile_bashrc(existing: &str) -> (String, ReconcileAction) {
    let stripped_managed = strip_managed_block(existing);
    let had_managed = stripped_managed.len() != existing.len();

    // Also strip old phs markers
    let stripped_old = strip_old_phs_block(&stripped_managed);
    let had_legacy = stripped_old.len() != stripped_managed.len();

    let mut content = stripped_old;
    while content.ends_with("\n\n") { content.pop(); }
    if !content.is_empty() && !content.ends_with('\n') { content.push('\n'); }
    content.push_str(ALIAS_BLOCK);

    if content == existing {
        return (content, ReconcileAction::Unchanged);
    }
    let action = match (had_managed, had_legacy) {
        (_, true) => ReconcileAction::Migrated,
        (true, false) => ReconcileAction::Updated,
        (false, false) => ReconcileAction::Installed,
    };
    (content, action)
}

fn strip_managed_block(input: &str) -> String {
    // Strip both new prelik and old phs BEGIN/END markers
    strip_block_between(input, ALIAS_BEGIN, ALIAS_END)
}

fn strip_old_phs_block(input: &str) -> String {
    // Old phs markers
    let old_begin = "# BEGIN: phs-shell-setup v1";
    let old_end = "# END: phs-shell-setup v1";
    let result = strip_block_between(input, old_begin, old_end);
    // Also strip legacy unmarked blocks ending with `unset _n`
    strip_legacy_tmux_loops(&result)
}

fn strip_block_between(input: &str, begin: &str, end: &str) -> String {
    let Some(begin_pos) = input.find(begin) else {
        return input.to_string();
    };
    let block_start = input[..begin_pos].rfind('\n').map_or(0, |i| i + 1);
    let after_begin = &input[begin_pos..];
    let Some(rel_end) = after_begin.find(end) else {
        return input[..block_start].to_string();
    };
    let abs_end = begin_pos + rel_end + end.len();
    let tail_start = input[abs_end..].find('\n').map_or(input.len(), |i| abs_end + i + 1);
    let mut out = String::with_capacity(input.len());
    out.push_str(&input[..block_start]);
    out.push_str(&input[tail_start..]);
    out
}

fn strip_legacy_tmux_loops(input: &str) -> String {
    // Strip blocks: `# <korean comment about tmux>\nfor _n in $(seq 1 99); do\n...\ndone\nunset _n\n`
    let mut result = input.to_string();
    // Repeat until stable (handles multiple legacy blocks)
    loop {
        if let Some(pos) = result.find("for _n in $(seq 1 99); do") {
            // Find the preceding comment line
            let block_start = result[..pos].rfind('\n').map_or(0, |i| {
                let line_before = result[..i].rfind('\n').map_or(0, |j| j + 1);
                let line = &result[line_before..i];
                if line.trim().starts_with('#') { line_before } else { i + 1 }
            });
            if let Some(end_pos) = result[pos..].find("unset _n") {
                let abs_end = pos + end_pos + "unset _n".len();
                let tail_start = result[abs_end..].find('\n').map_or(result.len(), |i| abs_end + i + 1);
                result = format!("{}{}", &result[..block_start], &result[tail_start..]);
                continue;
            }
        }
        // Also strip `# proxmox-host-setup aliases` blocks
        if let Some(pos) = result.find("# proxmox-host-setup aliases") {
            let block_start = result[..pos].rfind('\n').map_or(0, |i| i + 1);
            if let Some(end_pos) = result[pos..].find("unset _n") {
                let abs_end = pos + end_pos + "unset _n".len();
                let tail_start = result[abs_end..].find('\n').map_or(result.len(), |i| abs_end + i + 1);
                result = format!("{}{}", &result[..block_start], &result[tail_start..]);
                continue;
            }
        }
        break;
    }
    result
}

// ---------------------------------------------------------------------------
// nvim-markdown (포트 from workspace/markdown_nvim.rs)
// ---------------------------------------------------------------------------

fn nvim_markdown_setup(profile: &MarkdownProfile) -> anyhow::Result<()> {
    println!("=== mdnvim 설정 ===\n");
    install_mdnvim_packages()?;
    install_mdnvim_wrapper()?;

    let home = home_dir()?;
    write_mdnvim_profile(&home, profile)?;
    bootstrap_mdnvim_plugins()?;

    println!("\n=== mdnvim 설정 완료 ===");
    println!("프로필: {}", profile.as_str());
    println!("실행: mdnvim README.md");
    Ok(())
}

fn install_mdnvim_packages() -> anyhow::Result<()> {
    let missing: Vec<&&str> = MDNVIM_PACKAGES.iter().filter(|p| !pkg_installed(p)).collect();
    if missing.is_empty() { return Ok(()); }
    let pkgs = missing.iter().map(|p| **p).collect::<Vec<_>>().join(" ");
    common::run_bash(&format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkgs}"))?;
    Ok(())
}

fn install_mdnvim_wrapper() -> anyhow::Result<()> {
    let script = "#!/usr/bin/env bash\nexport NVIM_APPNAME=mdnvim\nexec nvim \"$@\"\n";
    fs::write(MDNVIM_WRAPPER, script)?;
    common::run("chmod", &["755", MDNVIM_WRAPPER])?;
    println!("[mdnvim] wrapper 설치 완료: {MDNVIM_WRAPPER}");
    Ok(())
}

fn write_mdnvim_profile(home: &Path, profile: &MarkdownProfile) -> anyhow::Result<()> {
    let config_dir = home.join(format!(".config/{MDNVIM_APPNAME}"));
    fs::create_dir_all(&config_dir)?;
    let init_path = config_dir.join("init.lua");
    let content = render_init_lua(profile);
    fs::write(&init_path, content)?;
    println!("[mdnvim] profile 설치 완료 ({})", profile.as_str());
    Ok(())
}

fn bootstrap_mdnvim_plugins() -> anyhow::Result<()> {
    let cmd = "env NVIM_APPNAME=mdnvim nvim --headless '+Lazy! sync' +qa";
    if let Err(e) = common::run_bash(cmd) {
        eprintln!("[mdnvim] plugin sync 경고: {e}");
    } else {
        println!("[mdnvim] plugin sync 완료");
    }
    Ok(())
}

fn render_init_lua(profile: &MarkdownProfile) -> String {
    let preview_plugin = match profile {
        MarkdownProfile::Markview => markview_plugin(),
        MarkdownProfile::Minimal | MarkdownProfile::Autolist => render_markdown_plugin(),
    };
    let list_plugin = match profile {
        MarkdownProfile::Autolist => autolist_plugin(),
        MarkdownProfile::Minimal | MarkdownProfile::Markview => "",
    };

    format!(
        r#"{marker}
vim.g.mapleader = " "
vim.opt.number = true
vim.opt.relativenumber = true
vim.opt.wrap = true
vim.opt.linebreak = true
vim.opt.breakindent = true
vim.opt.termguicolors = true
vim.opt.conceallevel = 2
vim.opt.updatetime = 200

local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
if not vim.loop.fs_stat(lazypath) then
  vim.fn.system({{
    "git",
    "clone",
    "--filter=blob:none",
    "https://github.com/folke/lazy.nvim.git",
    "--branch=stable",
    lazypath,
  }})
end
vim.opt.rtp:prepend(lazypath)

require("lazy").setup({{
  {{
    "nvim-treesitter/nvim-treesitter",
    branch = "master",
    lazy = false,
    build = ":TSUpdate",
    config = function()
      local ts = require("nvim-treesitter")
      ts.setup({{
        install_dir = vim.fn.stdpath("data") .. "/site"
      }})
      vim.defer_fn(function()
        pcall(function()
          ts.install({{
            "lua",
            "vim",
            "vimdoc",
            "query",
            "markdown",
            "markdown_inline",
          }}):wait(300000)
        end)
      end, 0)
    end,
  }},
  {{
    "williamboman/mason.nvim",
    config = function()
      require("mason").setup()
    end,
  }},
  {{
    "WhoIsSethDaniel/mason-tool-installer.nvim",
    dependencies = {{ "williamboman/mason.nvim" }},
    config = function()
      require("mason-tool-installer").setup({{
        ensure_installed = {{ "marksman" }},
        auto_update = false,
        run_on_start = true,
        start_delay = 1000,
      }})
    end,
  }},
  {{
    "nvim-neo-tree/neo-tree.nvim",
    dependencies = {{
      "nvim-lua/plenary.nvim",
      "MunifTanjim/nui.nvim",
      "nvim-tree/nvim-web-devicons",
    }},
    config = function()
      require("neo-tree").setup({{
        close_if_last_window = true,
        filesystem = {{
          filtered_items = {{
            hide_dotfiles = false,
            hide_gitignored = false,
          }},
          follow_current_file = {{
            enabled = true,
          }},
          hijack_netrw_behavior = "open_current",
        }},
        window = {{
          position = "left",
          width = 32,
          mappings = {{
            ["q"] = "close_window",
          }},
        }},
      }})
      vim.keymap.set("n", "-", "<cmd>Neotree reveal left<cr>", {{ desc = "reveal in file tree" }})
      vim.keymap.set("n", "<leader>e", "<cmd>Neotree toggle left<cr>", {{ desc = "file explorer" }})
    end,
  }},
{preview_plugin}
{list_plugin}
}}, {{
  ui = {{
    border = "rounded",
  }},
}})

vim.api.nvim_create_autocmd("FileType", {{
  pattern = {{ "markdown", "quarto", "rmd" }},
  callback = function(ev)
    vim.bo[ev.buf].textwidth = 100
    vim.bo[ev.buf].shiftwidth = 2
    vim.bo[ev.buf].tabstop = 2
    vim.wo.wrap = true
    vim.wo.linebreak = true
    if vim.fn.executable("marksman") == 1 then
      local path = vim.api.nvim_buf_get_name(ev.buf)
      local root = vim.fs.dirname(vim.fs.find({{ ".git" }}, {{ path = path, upward = true }})[1] or "")
      vim.lsp.start({{
        name = "marksman",
        cmd = {{ "marksman", "server" }},
        root_dir = root ~= "" and root or vim.fn.getcwd(),
      }}, {{
        bufnr = ev.buf,
      }})
    end
  end,
}})

vim.api.nvim_create_autocmd("VimEnter", {{
  once = true,
  callback = function()
    if #vim.api.nvim_list_uis() > 0 and vim.fn.argc() == 0 then
      vim.cmd("Neotree filesystem reveal left")
    end
  end,
}})
"#,
        marker = MDNVIM_MARKER,
        preview_plugin = preview_plugin,
        list_plugin = list_plugin
    )
}

fn render_markdown_plugin() -> &'static str {
    r#"  {
    "MeanderingProgrammer/render-markdown.nvim",
    ft = { "markdown", "quarto", "rmd" },
    dependencies = { "nvim-treesitter/nvim-treesitter" },
    config = function()
      require("render-markdown").setup({})
    end,
  },
"#
}

fn markview_plugin() -> &'static str {
    r#"  {
    "OXY2DEV/markview.nvim",
    ft = { "markdown", "quarto", "rmd" },
    dependencies = { "nvim-treesitter/nvim-treesitter" },
    config = function()
      require("markview").setup({})
    end,
  },
"#
}

fn autolist_plugin() -> &'static str {
    r#"  {
    "gaoDean/autolist.nvim",
    ft = { "markdown", "quarto", "rmd" },
    config = function()
      require("autolist").setup()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = { "markdown", "quarto", "rmd" },
        callback = function(ev)
          local opts = { buffer = ev.buf }
          vim.keymap.set("i", "<CR>", "<CR><cmd>AutolistNewBullet<cr>", opts)
          vim.keymap.set("n", "o", "o<cmd>AutolistNewBullet<cr>", opts)
          vim.keymap.set("n", "O", "O<cmd>AutolistNewBulletBefore<cr>", opts)
          vim.keymap.set("n", "<leader>mx", "<cmd>AutolistToggleCheckbox<cr>", opts)
          vim.keymap.set("n", "<leader>mr", "<cmd>AutolistRecalculate<cr>", opts)
        end,
      })
    end,
  },
"#
}

// ---------------------------------------------------------------------------
// lxc-shell (포트 from workspace/lxc_shell.rs)
// ---------------------------------------------------------------------------

fn lxc_shell_setup(vmid: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 셸 환경 설정 ===\n");
    ensure_lxc_running(vmid)?;

    // 셸 도구 설치
    lxc_install_packages(vmid, SHELL_TOOLS)?;

    // GitHub 도구 설치
    for (name, repo) in LXC_GITHUB_TOOLS {
        let installed = lxc_exec(vmid, &[name, "--version"]).is_ok();
        if installed {
            println!("[{name}] 이미 설치됨");
            continue;
        }
        println!("[{name}] 설치 중...");
        let arch_raw = lxc_exec(vmid, &["uname", "-m"])?;
        let arch = match arch_raw.trim() {
            "x86_64" => "x86_64",
            "aarch64" | "arm64" => "arm64",
            other => { eprintln!("[{name}] 지원하지 않는 아키텍처: {other}"); continue; }
        };
        let script = format!(
            "TAG=$(curl -sL https://api.github.com/repos/{repo}/releases/latest | grep '\"tag_name\"' | head -1 | cut -d'\"' -f4) && \
             [ -n \"$TAG\" ] && VERSION=${{TAG#v}} && \
             curl -sL \"https://github.com/{repo}/releases/download/$TAG/{name}_${{VERSION}}_Linux_{arch}.tar.gz\" | tar -xzf - -C /usr/local/bin {name} && \
             chmod +x /usr/local/bin/{name} && echo \"OK $TAG\""
        );
        match lxc_exec(vmid, &["bash", "-c", &script]) {
            Ok(out) if out.contains("OK ") => {
                let tag = out.lines().last().unwrap_or("").replace("OK ", "");
                println!("[{name}] 설치 완료 ({tag})");
            }
            _ => eprintln!("[{name}] 설치 실패"),
        }
    }

    // .bashrc alias (base64 경유로 안전 전달)
    let bashrc = lxc_exec(vmid, &["cat", "/root/.bashrc"]).unwrap_or_default();
    let (next, action) = reconcile_bashrc(&bashrc);
    match action {
        ReconcileAction::Unchanged => println!("[shell] alias 최신 (skip)"),
        _ => {
            println!("[shell] {}", action.message());
            let b64 = base64_encode(next.as_bytes());
            lxc_exec(vmid, &["bash", "-c",
                &format!("echo '{b64}' | base64 -d > /root/.bashrc.prelik.new && mv /root/.bashrc.prelik.new /root/.bashrc")])?;
            println!("[shell] .bashrc 갱신 완료");
        }
    }

    // git author
    let git_name = helpers::read_host_env("GIT_AUTHOR_NAME");
    let git_email = helpers::read_host_env("GIT_AUTHOR_EMAIL");
    if !git_name.is_empty() {
        let current = lxc_exec(vmid, &["bash", "-c", "git config --global user.name 2>/dev/null"]).unwrap_or_default();
        if current.trim() != git_name {
            lxc_exec(vmid, &["git", "config", "--global", "user.name", &git_name])?;
            lxc_exec(vmid, &["git", "config", "--global", "user.email", &git_email])?;
            println!("[git] author 설정 완료 ({git_name} <{git_email}>)");
        } else {
            println!("[git] author 이미 설정됨 ({git_name})");
        }
    }

    println!("\n=== LXC {vmid} 셸 환경 설정 완료 ===");
    Ok(())
}

// ---------------------------------------------------------------------------
// lxc-tmux (포트 from workspace/lxc_tmux.rs)
// ---------------------------------------------------------------------------

fn lxc_tmux_setup(vmid: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} tmux 설정 ===\n");
    ensure_lxc_running(vmid)?;

    // 플러그인 설치
    lxc_exec(vmid, &["mkdir", "-p", "/root/.tmux/plugins"])?;
    for (name, url) in TMUX_PLUGINS {
        let plugin_dir = format!("/root/.tmux/plugins/{name}");
        let exists = lxc_exec(vmid, &["test", "-d", &plugin_dir]).is_ok();
        if exists {
            println!("[tmux] {name} 이미 설치됨");
            continue;
        }
        println!("[tmux] {name} 설치 중...");
        match lxc_exec(vmid, &["git", "clone", url, &plugin_dir]) {
            Ok(_) => println!("[tmux] {name} 설치 완료"),
            Err(_) => eprintln!("[tmux] {name} 설치 실패"),
        }
    }

    // .tmux.conf 배포
    let existing = lxc_exec(vmid, &["cat", "/root/.tmux.conf"]).unwrap_or_default();
    if existing.contains("prelik-workspace") {
        println!("[tmux] .tmux.conf 이미 설정됨");
    } else {
        println!("[tmux] .tmux.conf 배포 중...");
        let escaped = TMUX_CONF.replace('\'', "'\\''");
        lxc_exec(vmid, &["bash", "-c",
            &format!("printf '%s' '{escaped}' > /root/.tmux.conf")])?;
        println!("[tmux] .tmux.conf 배포 완료");
    }

    println!("\n=== LXC {vmid} tmux 설정 완료 ===");
    Ok(())
}

// ---------------------------------------------------------------------------
// status (포트 from workspace/mod.rs status())
// ---------------------------------------------------------------------------

fn status() {
    println!("=== workspace 상태 ===\n");

    let home = match home_dir() {
        Ok(h) => h,
        Err(e) => { eprintln!("HOME 미설정: {e}"); return; }
    };

    println!("[CLI 도구]");
    for pkg in SHELL_TOOLS {
        println!("  {} {pkg}", if pkg_installed(pkg) { "+" } else { "-" });
    }

    println!("\n[tmux]");
    let tmux_conf = home.join(".tmux.conf");
    println!("  .tmux.conf: {}", if tmux_conf.exists() { "+" } else { "- (prelik run workspace tmux-setup)" });

    let session_ok = common::run("tmux", &["has-session", "-t", TMUX_SESSION]).is_ok();
    println!("  세션 '{TMUX_SESSION}': {}", if session_ok { "+ 실행중" } else { "-" });

    if let Ok(out) = common::run_bash("tmux ls 2>/dev/null | wc -l") {
        println!("  tmux 세션 총: {}개", out.trim());
    }

    println!("\n[tmux 플러그인]");
    for (name, _) in TMUX_PLUGINS {
        let ok = home.join(format!(".tmux/plugins/{name}")).exists();
        println!("  {} {name}", if ok { "+" } else { "-" });
    }

    println!("\n[shell alias]");
    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        let content = fs::read_to_string(&bashrc).unwrap_or_default();
        let has_block = content.contains(ALIAS_BEGIN);
        println!("  managed block: {}", if has_block { "+" } else { "- (prelik run workspace shell-setup)" });
    } else {
        println!("  .bashrc: - (없음)");
    }
    let alias_file = home.join(".bashrc.d/prelik.sh");
    println!("  bashrc.d/prelik.sh: {}", if alias_file.exists() { "+" } else { "-" });

    println!("\n[mdnvim]");
    println!("  wrapper ({}): {}", MDNVIM_WRAPPER, if Path::new(MDNVIM_WRAPPER).exists() { "+" } else { "-" });
    let mdnvim_config = home.join(format!(".config/{MDNVIM_APPNAME}/init.lua"));
    println!("  config: {}", if mdnvim_config.exists() { "+" } else { "-" });
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn doctor() {
    println!("=== prelik-workspace doctor ===");
    for (name, cmd) in &[
        ("tmux", "tmux"),
        ("nvim", "nvim"),
        ("git", "git"),
        ("bat", "bat"),
        ("eza", "eza"),
        ("fd", "fd"),
        ("fzf", "fzf"),
        ("zoxide", "zoxide"),
        ("marksman", "marksman"),
        ("pct (LXC)", "pct"),
    ] {
        println!("  {} {name}", if common::has_cmd(cmd) { "+" } else { "-" });
    }
    println!("\n선택 도구 설치: sudo apt install -y bat eza fd-find fzf zoxide");
}

// ---------------------------------------------------------------------------
// LXC helpers
// ---------------------------------------------------------------------------

fn ensure_lxc_running(vmid: &str) -> anyhow::Result<()> {
    let status = common::run("pct", &["status", vmid])?;
    if !status.contains("running") {
        anyhow::bail!("LXC {vmid} 이 실행 중이 아닙니다 (status: {status})");
    }
    Ok(())
}

fn lxc_exec(vmid: &str, cmd: &[&str]) -> anyhow::Result<String> {
    let mut args = vec!["exec", vmid, "--"];
    args.extend_from_slice(cmd);
    common::run("pct", &args)
}

fn lxc_install_packages(vmid: &str, packages: &[&str]) -> anyhow::Result<()> {
    let pkgs = packages.join(" ");
    lxc_exec(vmid, &["bash", "-c",
        &format!("DEBIAN_FRONTEND=noninteractive apt-get update -qq && apt-get install -y -qq {pkgs}")])?;
    println!("[lxc] 패키지 설치 완료: {pkgs}");
    Ok(())
}

/// Pure base64 encoder (RFC 4648).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() >= 2 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() >= 3 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// common helpers
// ---------------------------------------------------------------------------

fn home_dir() -> anyhow::Result<std::path::PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("HOME 미설정"))
}
