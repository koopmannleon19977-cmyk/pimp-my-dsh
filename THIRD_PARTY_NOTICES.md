# Third-Party Notices

`pimp-my-dsh` is licensed under the [MIT License](LICENSE). It is a distribution
that composes and hardens [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
(`@deepseek-ai/dsh`) as an exact npm dependency. It does not fork, vendor, or
copy upstream source code.

Each project below remains under its own license. Nothing in this file changes
those terms.

## Upstream: DeepSeek Harness

`pimp-my-dsh` depends on `@deepseek-ai/dsh@0.1.0-rc.6` and its first-party
`@deepseek-ai/dsh-*` companion packages, all pinned to the exact version
`0.1.0-rc.6`. These packages are published by DeepSeek AI under the MIT License.

The upstream MIT license text, reproduced verbatim from the upstream repository
(`LICENSE`, `master` branch):

```
MIT License

Copyright (c) 2026 DeepSeek

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Upstream repository: <https://github.com/deepseek-ai/deepseek-harness> (MIT).

## Distribution setup dependency

`pimp-my-dsh` includes [`pnpm@11.7.0`](https://github.com/pnpm/pnpm) (MIT) as
an exact runtime dependency. The CLI invokes its JavaScript entry directly for
portable, shell-free profile installation; it does not rely on Corepack being
bundled with the host Node.js release.

## Browser automation dependencies

The opt-in browser capability uses
[`@playwright/mcp@0.0.79`](https://github.com/microsoft/playwright-mcp)
and its Playwright runtime, published by Microsoft under the Apache License
2.0. It is connected through the first-party
`@deepseek-ai/dsh-mcp-client@0.1.0-rc.6` (MIT). The integration starts the
locally installed Google Chrome channel and does not redistribute a browser
binary.

## Direct runtime dependencies of `@deepseek-ai/dsh@0.1.0-rc.6`

The upstream CLI package declares the following non-`@deepseek-ai` direct
runtime dependencies:

| Package | License |
| --- | --- |
| [`commander`](https://github.com/tj/commander.js) | MIT |
| [`js-yaml`](https://github.com/nodeca/js-yaml) | MIT |
| [`node-addon-require-builtin`](https://www.npmjs.com/package/node-addon-require-builtin) | MIT |

`@deepseek-ai/cordis` (MIT) is the upstream source-vendored republish of the
[Cordis](https://github.com/cordiverse/cordis) framework. The remaining
`@deepseek-ai/dsh-*` packages are first-party upstream packages, all MIT.

## Transitive closure

The complete npm transitive closure — including native packages such as
`koffi` (MIT) and `node-pty` (MIT) that the upstream Windows sandbox and
terminal backends pull in — is recorded with exact pinned versions in
`pnpm-lock.yaml`. Inspect it with:

```sh
pnpm licenses list
```

Upstream also publishes its own generated notice file at
`THIRD_PARTY_NOTICES.md` in the `deepseek-harness` repository, which documents
its full workspace dependency closure. Because `pimp-my-dsh` consumes upstream
as a published npm artifact rather than a source checkout, `pnpm-lock.yaml` in
this repository is the authority on the exact installed closure.

## No copied upstream code

`pimp-my-dsh` contains no copied DeepSeek Harness source. All upstream
functionality is consumed through the published npm packages. The only
distribution-owned code is the root plugin (`src/plugin.ts`) and the CLI
(`src/cli.ts`), both original works under this repository's MIT license.

## Desktop supervisor runtime

The Windows desktop control surface (`apps/desktop`) bundles and references the
following additional components at runtime:

| Component | Version | License |
| --- | --- | --- |
| [Node.js](https://nodejs.org/) runtime (`node.exe`) | 24.19.0 (win-x64) | MIT (binary distribution aggregates additional per-component notices) |
| [Tauri](https://tauri.app/) framework and Rust crates (`tauri`, `tao`, `wry`) | 2.11.5 | MIT OR Apache-2.0 |
| [Tauri Single Instance plugin](https://github.com/tauri-apps/plugins-workspace/tree/single-instance-v2.4.3/plugins/single-instance) | 2.4.3 | MIT OR Apache-2.0 |
| [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) | evergreen (bootstrapped at install time) | Microsoft Software License Terms (not redistributed by this project) |
| [NSIS](https://nsis.sourceforge.io/) installer runtime | 3.x | zlib/libpng |
| [React](https://react.dev/) | 19.2.8 | MIT |
| [Fluent UI React Components](https://react.fluentui.dev/) (`@fluentui/react-components`) | 9.74.6 | MIT |
| [Fluent UI React Icons](https://github.com/microsoft/fluentui-system-icons) (`@fluentui/react-icons`) | 2.0.337 | MIT |
| [Vite](https://vite.dev/) build tooling | 8.2.1 | MIT |
| [TypeScript](https://www.typescriptlang.org/) compiler | 6.0.3 | Apache-2.0 |

The supervisor bundles its own copy of the Node.js runtime, downloaded from the
official `nodejs.org` distribution archive and SHA-256-pinned at build time; it
never resolves Node from `PATH` at runtime. WebView2 is installed by the NSIS
installer's `downloadBootstrapper` mode and is serviced by Microsoft, not shipped
by this repository.
