// 参考 since2006/shell-rs @ b2a307a: src/explorer/preview.rs 改写
// 改造：走 `DialogLayer`；工具放头部（原型 `previewDlg`）；「下载…」叠在预览上面、不关预览；「编辑」加上 SVG。
//! 预览：图片和 Markdown，用一个占窗口大部分的对话框（spec 第 6 节）。关了什么都不留。

use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::format_size;
use super::image_preview::{ImageView, Zoom, percent};
use crate::app::shortcuts::IMAGE_PREVIEW_CONTEXT;
use crate::app::{
    ActualSizePreview, CatalogIcon, FitPreview, SftpAction, SftpDispatch as _, ZoomPreviewIn,
    ZoomPreviewOut,
};
use crate::i18n::t;
use crate::settings::segmented;
use crate::theme::Tokens;
use crate::ui::DialogLayer;

/// 预览读的图片不能比这大。
pub const IMAGE_LIMIT: u64 = 20 * 1024 * 1024;

/// 文件能怎么预览，看文件名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Image(ImageFormat),
    Markdown,
}

impl PreviewKind {
    /// `path` 是文件名或完整路径。
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let (_, extension) = name.rsplit_once('.')?;
        Some(match extension.to_ascii_lowercase().as_str() {
            "png" => Self::Image(ImageFormat::Png),
            "jpg" | "jpeg" => Self::Image(ImageFormat::Jpeg),
            "gif" => Self::Image(ImageFormat::Gif),
            "webp" => Self::Image(ImageFormat::Webp),
            "svg" => Self::Image(ImageFormat::Svg),
            "bmp" => Self::Image(ImageFormat::Bmp),
            "ico" => Self::Image(ImageFormat::Ico),
            "tif" | "tiff" => Self::Image(ImageFormat::Tiff),
            "md" | "markdown" => Self::Markdown,
            _ => return None,
        })
    }

    /// 只能预览、不能编辑：图片不是文本。SVG 是文本（XML），和 Markdown 一样双击编辑、右键预览。
    pub fn preview_only(self) -> bool {
        matches!(self, Self::Image(format) if format != ImageFormat::Svg)
    }
}

/// 预览里显示的东西。
pub(super) enum PreviewContent {
    Image(Image),
    Markdown(SharedString),
}

/// 打开预览要的东西。
pub(super) struct Preview {
    pub name: SharedString,
    /// 头部的灰字开头：文件在哪。
    pub place: SharedString,
    /// 文件的字节数。
    pub bytes: u64,
    pub content: PreviewContent,
    /// 「下载…」：只有远程文件有。
    pub download: Option<SftpAction>,
    /// 「编辑」：只有 Markdown 和 SVG 有。
    pub edit: Option<SftpAction>,
    /// 窗口壳的焦点：上面两条命令经它派发。
    pub dispatch: FocusHandle,
}

enum PreviewBody {
    Image(ImageView),
    Markdown(SharedString),
}

/// 预览对话框的内容。每次打开新建一个，关掉就释放。
pub(super) struct PreviewDialog {
    name: SharedString,
    place: SharedString,
    bytes: u64,
    body: PreviewBody,
    download: Option<SftpAction>,
    edit: Option<SftpAction>,
    dispatch: FocusHandle,
    layer: DialogLayer,
    /// 打开之前焦点在哪（文件列表）：关掉时还回去。
    return_focus: Option<FocusHandle>,
    focus_handle: FocusHandle,
    _decode: Option<Task<()>>,
}

impl PreviewDialog {
    fn new(
        preview: Preview,
        layer: DialogLayer,
        return_focus: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) -> Self {
        let Preview {
            name,
            place,
            bytes,
            content,
            download,
            edit,
            dispatch,
        } = preview;
        let (body, decode) = match content {
            PreviewContent::Image(image) => {
                let view = ImageView::new(image.format());
                (PreviewBody::Image(view), Some(decode(image, cx)))
            }
            PreviewContent::Markdown(text) => (PreviewBody::Markdown(text), None),
        };
        // 解码好的图片一直占着窗口的纹理，要亲手丢掉。
        cx.on_release(|this, cx| {
            if let PreviewBody::Image(image) = &mut this.body
                && let Some(Ok(decoded)) = image.decoded.take()
            {
                cx.defer(move |cx| cx.drop_image(decoded, None));
            }
        })
        .detach();
        Self {
            name,
            place,
            bytes,
            body,
            download,
            edit,
            dispatch,
            layer,
            return_focus,
            focus_handle: cx.focus_handle(),
            _decode: decode,
        }
    }

    /// 对图片的状态做点什么，画面跟着变。Markdown 什么都不做。
    pub(super) fn with_image(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ImageView),
    ) {
        if let PreviewBody::Image(image) = &mut self.body {
            change(image);
            cx.notify();
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.close(window, cx);
        if let Some(focus) = &self.return_focus {
            window.focus(focus, cx);
        }
    }

    /// 「下载…」：确认框叠在预览上面，取消了回到预览。
    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(action) = &self.download {
            self.dispatch.dispatch_sftp_action(action, window, cx);
        }
    }

    /// 「编辑」：关闭预览，用编辑器标签打开，已经开着就切过去。
    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        if let Some(action) = &self.edit {
            self.dispatch.dispatch_sftp_action(action, window, cx);
        }
    }

    /// 头部的灰字：文件在哪、多大，图片还有多少像素。
    fn info(&self) -> SharedString {
        let size = format_size(self.bytes);
        match &self.body {
            PreviewBody::Image(image) => match image.pixels() {
                Some(pixels) => t!(
                    "preview.info_image",
                    place = self.place,
                    size = size,
                    width = f32::from(pixels.width) as u32,
                    height = f32::from(pixels.height) as u32
                ),
                None => t!("preview.info", place = self.place, size = size),
            },
            PreviewBody::Markdown(_) => t!("preview.info", place = self.place, size = size),
        }
    }

    /// 图片的工具：「适合窗口」「原图」、缩小、倍数、放大。
    fn image_tools(&self, image: &ImageView, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let this = cx.entity().downgrade();
        let choose_mode = std::rc::Rc::new(move |mode: usize, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |this, cx| {
                let zoom = if mode == 0 {
                    Zoom::Fit
                } else {
                    Zoom::Scale(1.0)
                };
                this.with_image(cx, |image| image.set_zoom(zoom));
            });
        });
        let zoom: SharedString = image.scale().map(percent).unwrap_or_default().into();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .child(segmented(
                "preview-zoom-mode",
                vec![t!("preview.fit"), t!("preview.actual_size")],
                image.mode(),
                choose_mode,
                tokens,
            ))
            .child(tool_icon(
                "preview-zoom-out",
                CatalogIcon::ZoomOut,
                t!("preview.zoom_out"),
                tokens,
                cx.listener(|this, _, _, cx| this.with_image(cx, ImageView::step_out)),
            ))
            .child(
                div()
                    .id("preview-zoom")
                    .test_support()
                    .aria_label(zoom.clone())
                    .w(px(40.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(tokens.faint)
                    .child(zoom),
            )
            .child(tool_icon(
                "preview-zoom-in",
                CatalogIcon::ZoomIn,
                t!("preview.zoom_in"),
                tokens,
                cx.listener(|this, _, _, cx| this.with_image(cx, ImageView::step_in)),
            ))
    }

    /// 标题行（原型 `.dh`）：文件名、灰字、工具、「编辑」「下载…」、关闭。
    fn header(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let tools = match &self.body {
            PreviewBody::Image(image) => Some(self.image_tools(image, tokens, cx)),
            PreviewBody::Markdown(_) => None,
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(46.))
            .pl(px(18.))
            .pr(px(8.))
            .child(
                div()
                    .min_w_0()
                    .flex_shrink_0()
                    .max_w(relative(0.4))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.name.clone()),
            )
            .child({
                let info = self.info();
                div()
                    .id("preview-info")
                    .test_support()
                    .aria_label(info.clone())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .text_color(tokens.faint)
                    .child(info)
            })
            .children(tools)
            .when(self.edit.is_some(), |header| {
                header.child(tool_button(
                    "preview-edit",
                    CatalogIcon::SquarePen,
                    t!("preview.edit"),
                    tokens,
                    cx.listener(|this, _, window, cx| this.edit(window, cx)),
                ))
            })
            .when(self.download.is_some(), |header| {
                header.child(tool_button(
                    "preview-download",
                    CatalogIcon::Download,
                    t!("preview.download"),
                    tokens,
                    cx.listener(|this, _, window, cx| this.download(window, cx)),
                ))
            })
            .child(div().ml(px(6.)).child(tool_icon(
                "preview-close",
                IconName::Close,
                t!("preview.close"),
                tokens,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )))
    }
}

impl Render for PreviewDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::current(cx);
        let header = self.header(&tokens, cx);
        let is_image = matches!(self.body, PreviewBody::Image(_));
        let body = match &self.body {
            PreviewBody::Image(image) => image.render_frame(&tokens, window, cx).into_any_element(),
            PreviewBody::Markdown(text) => render_markdown(&self.name, text).into_any_element(),
        };
        div()
            .id("preview-dialog")
            .test_support()
            .aria_label(self.name.clone())
            .when(is_image, |root| root.key_context(IMAGE_PREVIEW_CONTEXT))
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|this, _: &ZoomPreviewIn, _, cx| {
                    this.with_image(cx, ImageView::step_in)
                }),
            )
            .on_action(cx.listener(|this, _: &ZoomPreviewOut, _, cx| {
                this.with_image(cx, ImageView::step_out)
            }))
            .on_action(cx.listener(|this, _: &FitPreview, _, cx| {
                this.with_image(cx, |image| image.set_zoom(Zoom::Fit))
            }))
            .on_action(cx.listener(|this, _: &ActualSizePreview, _, cx| {
                this.with_image(cx, |image| image.set_zoom(Zoom::Scale(1.0)))
            }))
            .flex()
            .flex_col()
            .size_full()
            .text_color(tokens.text)
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(tokens.line)
                    .child(body),
            )
    }
}

/// 排版后的 Markdown（原型 `.pv .md`）。点链接默认用浏览器打开。
fn render_markdown(name: &SharedString, text: &SharedString) -> impl IntoElement + use<> {
    div()
        .id("preview-markdown")
        .test_support()
        .aria_label(name.clone())
        .flex_1()
        .min_w_0()
        .min_h_0()
        .size_full()
        .text_size(px(13.5))
        .child(
            TextView::markdown("preview-markdown-text", text.clone())
                .scrollable(true)
                .selectable(true)
                .size_full()
                .px(px(60.))
                .py(px(22.)),
        )
}

/// 在后台解码，解不开就说明，别让 `img` 一直转。
fn decode(image: Image, cx: &mut Context<PreviewDialog>) -> Task<()> {
    let renderer = cx.svg_renderer();
    cx.spawn(async move |this, cx| {
        let decoded = cx
            .background_spawn(async move { image.to_image_data(renderer) })
            .await
            .map_err(|error| format!("{error:#}"));
        let _ = this.update(cx, |this, cx| {
            this.with_image(cx, |image| image.decoded = Some(decoded));
        });
    })
}

/// 头部的图标按钮（原型 `.ib`）。
fn tool_icon<I: Into<Icon>, F: Fn(&ClickEvent, &mut Window, &mut App) + 'static>(
    id: &'static str,
    icon: I,
    label: SharedString,
    tokens: &Tokens,
    on_click: F,
) -> impl IntoElement + use<I, F> {
    let (hover, text) = (tokens.hover, tokens.text);
    div()
        .id(id)
        .test_support()
        .aria_label(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(26.))
        .rounded(px(10.))
        .text_color(tokens.icon)
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(text))
        .on_click(on_click)
        .child(icon.into().size(px(15.)))
}

/// 头部带字的按钮（原型 `.btn.sm`）。
fn tool_button<F: Fn(&ClickEvent, &mut Window, &mut App) + 'static>(
    id: &'static str,
    icon: CatalogIcon,
    label: SharedString,
    tokens: &Tokens,
    on_click: F,
) -> impl IntoElement + use<F> {
    let pressed = tokens.pressed;
    div()
        .id(id)
        .test_support()
        .aria_label(label.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(10.))
        .rounded(px(13.))
        .bg(tokens.hover)
        .text_size(px(12.5))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(pressed))
        .on_click(on_click)
        .child(Icon::new(icon).size(px(14.)))
        .child(label)
}

/// 打开预览：焦点给预览，缩放的按键马上能用；关掉时还给文件列表。
pub(super) fn open_preview_dialog(
    preview: Preview,
    layer: DialogLayer,
    window: &mut Window,
    cx: &mut App,
) {
    let return_focus = window.focused(cx);
    let view = cx.new(|cx| PreviewDialog::new(preview, layer.clone(), return_focus.clone(), cx));
    let focus = view.read(cx).focus_handle.clone();
    layer.open_dismissable(
        move |dialog, window, _| {
            // 原型 `.pv`：四周留白，占窗口大部分。
            let viewport = window.viewport_size();
            dialog
                .w((viewport.width - px(120.)).max(px(320.)))
                .h((viewport.height - px(80.)).max(px(320.)))
                .margin_top(px(40.))
                .p_0()
                .rounded(px(14.))
                .close_button(false)
                .overlay_closable(false)
                .child(view.clone())
        },
        move |window, cx| {
            if let Some(focus) = &return_focus {
                window.focus(focus, cx);
            }
        },
        window,
        cx,
    );
    // 要在打开的同一次更新里给焦点，否则对话框自己的焦点会抢过去。
    window.focus(&focus, cx);
}

#[cfg(test)]
mod tests {
    use super::PreviewKind;
    use gpui_kit::ImageFormat;

    #[test]
    fn images_and_markdown_are_previewed_by_their_names() {
        for (name, format) in [
            ("a.png", ImageFormat::Png),
            ("photo.JPG", ImageFormat::Jpeg),
            ("a.jpeg", ImageFormat::Jpeg),
            ("a.gif", ImageFormat::Gif),
            ("a.webp", ImageFormat::Webp),
            ("logo.svg", ImageFormat::Svg),
            ("a.bmp", ImageFormat::Bmp),
            ("favicon.ico", ImageFormat::Ico),
            ("scan.tif", ImageFormat::Tiff),
            ("scan.tiff", ImageFormat::Tiff),
        ] {
            assert_eq!(PreviewKind::of(name), Some(PreviewKind::Image(format)));
        }
        assert_eq!(PreviewKind::of("README.md"), Some(PreviewKind::Markdown));
        assert_eq!(
            PreviewKind::of("notes.markdown"),
            Some(PreviewKind::Markdown)
        );
        assert_eq!(PreviewKind::of("archive.tar.gz"), None);
        assert_eq!(PreviewKind::of("Makefile"), None);
    }

    #[test]
    fn only_the_last_part_of_a_path_counts() {
        assert_eq!(PreviewKind::of("/etc/a.png/Makefile"), None);
        assert_eq!(
            PreviewKind::of(r"C:\Users\图.d\图.PNG"),
            Some(PreviewKind::Image(ImageFormat::Png))
        );
    }

    #[test]
    fn pictures_are_only_previewed_but_svg_and_markdown_are_edited_too() {
        assert!(PreviewKind::of("a.png").unwrap().preview_only());
        assert!(!PreviewKind::of("a.svg").unwrap().preview_only());
        assert!(!PreviewKind::Markdown.preview_only());
    }
}
