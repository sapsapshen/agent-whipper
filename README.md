# AgentWhipper

[简体中文](#zh-cn) | [English](#english) | [Español](#espanol)

---

<a id="zh-cn"></a>
## 简体中文

### 项目简介

AgentWhipper 是一个受 OpenWhip 项目启发的，用于监督和干预 AI 编码智能体的命令行工具，目标是：

- 启动并监控新的 Agent 进程
- 基于输出活动与 CPU 变化识别运行中 / idle / stalled / zombie
- 通过预设提示词或控制信号尝试打断、提醒、加速
- 记录成功干预统计

灵感来自 OpenWhip。

### 当前实际状态

以下内容按**当前代码**与**最近实际验证**整理，不写计划中的能力。

#### 已可用

- `whip start <agent> --mode watch`
  - 会启动并监控新 Agent 进程
  - 当前支持的启动目标：`codex` / `claude` / `hermes`
- `whip watch --all`
  - 会自动探测并持续显示所有可检测到的 agent 运行时
  - 不依赖是否安装 `codex`
- 输出 + CPU 双重状态判定
- 内置预设 + YAML 自定义预设
- `wait` 步骤支持：
  - `duration`
  - `duration_secs`
- 启动脚本：
  - `start-whip.bat`
  - `start-whip.sh`
  - 每次都会重新构建最新 `release`
  - 菜单为**单键立即执行**，不需要再按回车
- 运行时检测目录已扩展到常见 Agent / IDE / 扩展进程

#### 已实现到代码中的检测目录

至少包含：

- OpenClaw
- Hermes
- OpenCode
- Visual Studio Code
- GitHub Copilot
- Cursor
- Visual Studio Code Insiders
- Trae
- Trae Solo
- Zed
- Claude Code
- Claude Desktop
- Codex CLI
- Codex Desktop

以及更多常见工具：

- Windsurf
- Continue
- Cline
- Roo Code
- Kilo Code
- Aider
- Gemini CLI
- Qwen Code
- Sourcegraph Cody
- Tabby
- Kiro
- Void
- PearAI
- Replit Agent
- Warp
- oh-my-codex

### 当前限制

这些限制是**当前真实存在**的：

- `attach` / `watch` / `inject` / `status`
  仍未实现真实跨进程会话管理
- `Exec` 类型预设步骤默认禁用

### 最近一次实际验证结果

- `cargo test`：通过
- `cargo build --release`：通过
- Windows 启动菜单单键执行：通过
- `whip whip --preset speedup`：会扫描运行时并尝试真实注入

也就是说：

**`handle_whip` 主入口现在会走真实检测与注入链路，并在成功后写入统计。**

### 快速开始

```bash
cargo build --release
```

常用命令：

```bash
whip start codex --mode watch
whip watch --all
whip stats
whip whip --preset speedup
whip preset list
```

### 预设示例

```yaml
name: speedup
description: 加速
trigger_on: [STALLED]
max_retries: 5
steps:
  - type: text
    content:
      - "加快速度。不要过度思考，先给能工作的代码，再优化。跳过不必要的解释。"
      - "别磨蹭了，先上能跑的代码，解释后面补上。"
  - type: enter
```

---

<a id="english"></a>
## English

### Overview

AgentWhipper is a CLI tool for supervising and intervening in AI coding agents. Its goals are:

- launch and monitor new agent processes
- classify running / idle / stalled / zombie states from output + CPU activity
- inject preset prompts or control signals to interrupt, remind, or speed up agents
- record successful intervention stats

Inspired by OpenWhip.

### Current real status

The notes below reflect the **current codebase** and **recent observed behavior**, not planned features.

#### Working today

- `whip start <agent> --mode watch`
  - launches and monitors a new agent process
  - currently supported startup targets: `codex`, `claude`, `hermes`
- `whip watch --all`
  - auto-detects and continuously reports all detectable agent runtimes
  - does not require `codex` to be installed
- output + CPU based state detection
- built-in presets + YAML custom presets
- `wait` preset step accepts:
  - `duration`
  - `duration_secs`
- launcher scripts:
  - `start-whip.bat`
  - `start-whip.sh`
  - always rebuild the latest `release`
  - menu options execute on a **single key press**
- runtime detection catalog has been expanded to many common agent / IDE / extension processes

#### Runtime detection catalog currently included in code

At minimum:

- OpenClaw
- Hermes
- OpenCode
- Visual Studio Code
- GitHub Copilot
- Cursor
- Visual Studio Code Insiders
- Trae
- Trae Solo
- Zed
- Claude Code
- Claude Desktop
- Codex CLI
- Codex Desktop

Plus additional common tools:

- Windsurf
- Continue
- Cline
- Roo Code
- Kilo Code
- Aider
- Gemini CLI
- Qwen Code
- Sourcegraph Cody
- Tabby
- Kiro
- Void
- PearAI
- Replit Agent
- Warp
- oh-my-codex

### Current limitations

These limitations are **real right now**:

- `attach` / `watch` / `inject` / `status`
  still do not provide real cross-process session management
- `Exec` preset steps remain disabled by default

### Latest observed validation

- `cargo test`: passed
- `cargo build --release`: passed
- Windows single-key launcher menu: passed
- `whip whip --preset speedup`: scans runtimes and attempts live injection

So the real status is:

**the `handle_whip` entrypoint now uses the real detection and injection path and records successful interventions.**

### Quick start

```bash
cargo build --release
```

Common commands:

```bash
whip start codex --mode watch
whip watch --all
whip stats
whip whip --preset speedup
whip preset list
```

---

<a id="espanol"></a>
## Español

### Resumen

AgentWhipper es una herramienta CLI para supervisar e intervenir en agentes de programación con IA. Sus objetivos son:

- iniciar y monitorear nuevos procesos de agentes
- clasificar estados running / idle / stalled / zombie con salida + CPU
- inyectar prompts predefinidos o señales de control para interrumpir, recordar o acelerar
- registrar estadísticas de intervenciones exitosas

Inspirado en OpenWhip.

### Estado real actual

Lo siguiente refleja el **estado actual del código** y el **comportamiento observado recientemente**, no funciones planeadas.

#### Funciona hoy

- `whip start <agent> --mode watch`
  - inicia y monitorea un nuevo proceso de agente
  - objetivos de arranque soportados actualmente: `codex`, `claude`, `hermes`
- detección de estado por salida + CPU
- presets integrados + presets YAML personalizados
- el paso `wait` acepta:
  - `duration`
  - `duration_secs`
- scripts de arranque:
  - `start-whip.bat`
  - `start-whip.sh`
  - siempre recompilan la versión `release` más reciente
  - el menú ejecuta con **una sola tecla**
- el catálogo de detección de runtimes se amplió a muchos procesos comunes de agentes / IDE / extensiones

#### Catálogo de detección incluido actualmente en el código

Como mínimo:

- OpenClaw
- Hermes
- OpenCode
- Visual Studio Code
- GitHub Copilot
- Cursor
- Visual Studio Code Insiders
- Trae
- Trae Solo
- Zed
- Claude Code
- Claude Desktop
- Codex CLI
- Codex Desktop

Y además otras herramientas comunes:

- Windsurf
- Continue
- Cline
- Roo Code
- Kilo Code
- Aider
- Gemini CLI
- Qwen Code
- Sourcegraph Cody
- Tabby
- Kiro
- Void
- PearAI
- Replit Agent
- Warp
- oh-my-codex

### Limitaciones actuales

Estas limitaciones son **reales en este momento**:

- `attach` / `watch` / `inject` / `status`
  todavía no ofrecen gestión real de sesiones entre procesos
- los pasos `Exec` siguen deshabilitados por defecto

### Última validación observada

- `cargo test`: aprobado
- `cargo build --release`: aprobado
- menú de arranque Windows con una sola tecla: aprobado
- `whip whip --preset speedup`: escanea runtimes e intenta inyección real

En resumen:

**el punto de entrada `handle_whip` ahora usa la ruta real de detección e inyección y registra las intervenciones exitosas.**

### Inicio rápido

```bash
cargo build --release
```

Comandos comunes:

```bash
whip start codex --mode watch
whip stats
whip whip --preset speedup
whip preset list
```

---

*Inspired by OpenWhip, built with Rust.*
