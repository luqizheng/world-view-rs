# wv-app 打包字体

> 界面字体（ADR-0019）：**拉丁 Inter、中文 Noto Sans SC**，`theme/fonts.rs`
> 以 `include_bytes!` 编译期嵌入，经 `LoadedFonts` 注册进 egui 与 Bevy。
> 本目录只放 **正文字重（Regular）**——注册位当前只消费这两个文件。

## 文件

| 文件 | 字体 | 版本（name 表） | 许可 | 用途 |
|---|---|---|---|---|
| `Inter-Regular.ttf` | Inter Regular | 4.001 | SIL OFL-1.1 | 拉丁（比例族 `Proportional` 首位） |
| `NotoSansSC-Regular.ttf` | Noto Sans SC Regular | 2.004 | SIL OFL-1.1 | 中文（比例族回退）+ Bevy `Text2d` 地名标注 |

两个字体均为 **SIL Open Font License 1.1**（可自由嵌入 / 再分发；
许可全文见上游仓库，如 [`rsms/inter`](https://github.com/rsms/inter/blob/master/LICENSE.txt)
与 [`notofonts/noto-cjk`](https://github.com/notofonts/noto-cjk/blob/main/LICENSE)）。

## 来源

经 jsDelivr 从 `@expo-google-fonts` 镜像取 **静态字重实例**（非可变字体）：

```bash
# Inter 4.x（路径 = 字重目录/文件名）
curl -L -o Inter-Regular.ttf \
  "https://cdn.jsdelivr.net/npm/@expo-google-fonts/inter@0.4.2/400Regular/Inter_400Regular.ttf"

# Noto Sans SC 2.004
curl -L -o NotoSansSC-Regular.ttf \
  "https://cdn.jsdelivr.net/npm/@expo-google-fonts/noto-sans-sc@0.4.4/400Regular/NotoSansSC_400Regular.ttf"
```

> 直连 GitHub Releases（Inter-4.1.zip 约 32 MB / noto-cjk 打包 zip 约 50 MB）
> 在本机网络上极慢且多次截断；jsDelivr 单文件可断点重试，是更稳的获取路径。

## 追加字重（600 SemiBold 等，需要时）

设计稿 `ART_STYLE_SPEC` §4.2 的字重档为 **400 / 600**。当前工程只用 Regular
（egui 无合成粗体，`.strong()` 只改颜色）；要做 600 字重需：下载对应字重文件 →
`fonts.rs` 注册 egui **命名族**（如 `FontFamily::Name("semibold")`）→ 在面板标题
等位点切换 `FontId`。取文件（同源同路径规则）：

```bash
curl -L -o Inter-SemiBold.ttf \
  "https://cdn.jsdelivr.net/npm/@expo-google-fonts/inter@0.4.2/600SemiBold/Inter_600SemiBold.ttf"
curl -L -o NotoSansSC-SemiBold.ttf \
  "https://cdn.jsdelivr.net/npm/@expo-google-fonts/noto-sans-sc@0.4.4/600SemiBold/NotoSansSC_600SemiBold.ttf"
```

**注意体积**：Noto Sans SC 每个字重约 10 MB；追加前先确认是否真的需要
（中文 600 与 400 在 11–13 px 下视觉差有限，低分屏尤甚——§4.2 约束 4 的原始顾虑）。

## 已知覆盖缺口

◈(U+25C8) ◧(U+25E7) ⬡(U+2B21)（`state::Panel::icon` 面板头图标）在 Inter /
Noto Sans SC / egui 内置字体中均无字形，egui 以替换符 ◻(U+25FB) 呈现。
真修复走 SVG 图标化（`assets/ui/tool_icons/` 已有基建），见 ADR-0019 §4.1。
