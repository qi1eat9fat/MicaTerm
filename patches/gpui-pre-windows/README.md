# gpui-pre-windows（MicaTerm 补丁副本）

根 `Cargo.toml` 用 `[patch.crates-io]` 把 `gpui-pre-windows` 换成这份副本（[工单 30](../../.scratch/qishell-v1/issues/30-light-mica-alpha-fringe.md)）。

## 出处

- 上游：crates.io 上的 `gpui-pre-windows` **0.3.2**（checksum `682040da1444801c80ff0d885956467cc087429824c77aa9c0dfeb27e5105dfc`），是 Zed 的 `gpui_windows` crate 在 `zed-industries/zed@801c087af22dd189dc1aa49e2f370b4f04190b19` 的快照。
- 许可：Apache-2.0，原样保留 `LICENSE-APACHE`。
- 拷贝了包里的 `Cargo.toml`、`build.rs`、`src/` 和许可文件，没带 `Cargo.lock`、`Cargo.toml.orig`。

## 改了什么

照上游 zed-industries/zed#65571（commit `e18de65`）在 DirectX 上的改法，`src/directx_renderer.rs` 改两行，`DestBlendAlpha` 都从 `D3D11_BLEND_ONE` 改成 `D3D11_BLEND_INV_SRC_ALPHA`（改动处有注释）：

1. `create_blend_state`：四边形、阴影、下划线、单色和彩色精灵（文字、图标）用的混合状态。
2. `create_blend_state_for_path_sprite`：把栅格化好的 path（预乘颜色）贴回画面用的混合状态。

## 为什么

交换链是 `DXGI_ALPHA_MODE_PREMULTIPLIED`。四边形、文字、图标用的这个混合状态，颜色是 `SRC_ALPHA / INV_SRC_ALPHA`（着色器输出非预乘颜色，写进去的是预乘结果，没问题），alpha 却是 `ONE / ONE`，两层 alpha 直接相加。窗口不透明时 alpha 本来就是 1，看不出来；开 Mica 后窗口底是 50% 的渐变，在上面画覆盖率为 c 的抗锯齿边缘，alpha 成了 `0.5 + c`（应是 `0.5 + 0.5c`），颜色却按正确的预乘值写入。DWM 合成时透出的 Mica 变少，浅色下字变重、圆角边缘一圈深色虚线。改成 `INV_SRC_ALPHA` 就是标准的 source-over，和同文件里 path 栅格化的混合状态一致。

上游问题：zed-industries/zed#56382「Fonts become blurry when Mica effect is enabled in GPUI」。上游已在 zed-industries/zed#65571（commit `e18de65`，2026-10-14，「gpui: Composite translucent layers with source-over alpha」）修好，crates.io 上 `gpui-pre-windows` 0.3.11 已带这个修复。path 精灵那处是同一个错：source 已是预乘颜色，alpha 也该按 source-over 合成。path 在 MicaTerm 里用于输入框和编辑器的选区、缩进参考线（`gpui-base` 的 input）和虚线分隔符（`gpui-component` 的 `Separator`）。两处都改，补丁和上游那次提交一一对应，以后好对照。

## 什么时候删掉

`gpui-kit` 0.6.1 经 `gpui-pre-platform` 把 `gpui-pre-windows` 钉在 `=0.3.2`，没法单独升级。等 `gpui-kit` 升到用 `gpui-pre` 0.3.11 及以上的版本，删掉这个目录和根 `Cargo.toml` 的 `[patch.crates-io]` 条目即可。
