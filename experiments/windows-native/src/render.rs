//! Direct2D + DirectWrite rendering for DBM's custom workbench chrome.

use std::collections::HashMap;

use windows::core::{Interface, Result, HSTRING};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    IDWriteTextLayout, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::theme::{self, Font};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Leading,
    Center,
}

/// A renderer that redraws the whole window each frame. The window is small
/// enough that a full repaint per input event is cheaper than tracking damage.
pub struct Renderer {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    /// A collection holding the embedded Satoshi font, when it is available.
    fonts: Option<IDWriteFontCollection>,
    target: Option<ID2D1HwndRenderTarget>,
    hwnd: HWND,
    dpi: f32,
    formats: HashMap<(u32, bool, bool), IDWriteTextFormat>,
    brushes: HashMap<(u32, u32), ID2D1SolidColorBrush>,
    clips: usize,
    transforms: Vec<Matrix3x2>,
    pub width: f32,
    pub height: f32,
}

impl Renderer {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let factory: ID2D1Factory = unsafe {
            D2D1CreateFactory(
                D2D1_FACTORY_TYPE_SINGLE_THREADED,
                Some(std::ptr::null::<D2D1_FACTORY_OPTIONS>()),
            )?
        };
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let fonts = load_satoshi(&dwrite);
        Ok(Self {
            factory,
            dwrite,
            fonts,
            target: None,
            hwnd,
            dpi: 1.0,
            formats: HashMap::new(),
            brushes: HashMap::new(),
            clips: 0,
            transforms: Vec::new(),
            width: 0.0,
            height: 0.0,
        })
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        if (self.dpi - dpi).abs() > f32::EPSILON {
            self.dpi = dpi;
            // DIP-based sizes change with DPI, so drop the cached target.
            self.target = None;
            self.formats.clear();
        }
    }

    pub fn dpi_scale(&self) -> f32 {
        self.dpi / 96.0
    }

    /// Starts a frame. Returns false when there is nothing to draw into.
    pub fn begin(&mut self, width: u32, height: u32) -> Result<bool> {
        if width == 0 || height == 0 {
            return Ok(false);
        }
        if self.target.is_none() {
            let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0 * self.dpi_scale(),
                dpiY: 96.0 * self.dpi_scale(),
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                    width: width.max(1),
                    height: height.max(1),
                },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };
            let target = unsafe {
                self.factory
                    .CreateHwndRenderTarget(&target_properties, &hwnd_properties)?
            };
            self.brushes.clear();
            self.target = Some(target);
        }
        let target = self.target.as_ref().expect("render target");
        unsafe {
            target.Resize(&windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                width: width.max(1),
                height: height.max(1),
            })?;
            target.BeginDraw();
            target.SetTransform(&Matrix3x2::identity());
            target.Clear(Some(&theme::color(theme::BG, 1.0)));
        }
        self.width = width as f32 / self.dpi_scale();
        self.height = height as f32 / self.dpi_scale();
        Ok(true)
    }

    pub fn end(&mut self) -> Result<()> {
        if let Some(target) = &self.target {
            unsafe { target.EndDraw(None, None)? };
        }
        Ok(())
    }

    fn brush(&mut self, rgb: u32, alpha: f32) -> Result<ID2D1SolidColorBrush> {
        let key = (rgb, (alpha * 255.0) as u32);
        if let Some(brush) = self.brushes.get(&key) {
            return Ok(brush.clone());
        }
        let target = self.target.clone().expect("render target");
        let brush = unsafe {
            target.CreateSolidColorBrush(
                &theme::color(rgb, alpha),
                Some(std::ptr::null::<
                    windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES,
                >()),
            )?
        };
        self.brushes.insert(key, brush.clone());
        Ok(brush)
    }

    fn format(&mut self, font: Font) -> Result<IDWriteTextFormat> {
        let key = (
            (font.size() * 10.0) as u32,
            font.bold(),
            font.family() == "Consolas",
        );
        if let Some(format) = self.formats.get(&key) {
            return Ok(format.clone());
        }
        let weight = if font.bold() {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let mut format = unsafe {
            self.dwrite.CreateTextFormat(
                &HSTRING::from(font.family()),
                self.fonts.as_ref(),
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font.size(),
                &HSTRING::from("en-us"),
            )
        };
        if format.is_err() && self.fonts.is_some() {
            // Wine's DirectWrite has no in-memory font loader; fall back to the
            // system collection rather than failing to draw text.
            format = unsafe {
                self.dwrite.CreateTextFormat(
                    &HSTRING::from(font.family()),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font.size(),
                    &HSTRING::from("en-us"),
                )
            };
        }
        let format = match format {
            Ok(format) => format,
            Err(error) => {
                eprintln!("CreateTextFormat failed: {error}");
                return Err(error);
            }
        };
        unsafe {
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
        }
        self.formats.insert(key, format.clone());
        Ok(format)
    }

    pub fn fill_rect(&mut self, rect: D2D_RECT_F, rgb: u32, alpha: f32) -> Result<()> {
        let brush = self.brush(rgb, alpha)?;
        let target = self.target.clone().expect("render target");
        unsafe { target.FillRectangle(&rect, &brush) };
        Ok(())
    }

    pub fn fill_round_rect(
        &mut self,
        rect: D2D_RECT_F,
        radius: f32,
        rgb: u32,
        alpha: f32,
    ) -> Result<()> {
        let brush = self.brush(rgb, alpha)?;
        let target = self.target.clone().expect("render target");
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { target.FillRoundedRectangle(&rounded, &brush) };
        Ok(())
    }

    pub fn stroke_round_rect(
        &mut self,
        rect: D2D_RECT_F,
        radius: f32,
        rgb: u32,
        width: f32,
    ) -> Result<()> {
        let brush = self.brush(rgb, 1.0)?;
        let target = self.target.clone().expect("render target");
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { target.DrawRoundedRectangle(&rounded, &brush, width, None) };
        Ok(())
    }

    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, rgb: u32) -> Result<()> {
        let brush = self.brush(rgb, 1.0)?;
        let target = self.target.clone().expect("render target");
        unsafe {
            target.DrawLine(
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: x0, y },
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: x1, y },
                &brush,
                1.0,
                None,
            );
        }
        Ok(())
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, rgb: u32) -> Result<()> {
        let brush = self.brush(rgb, 1.0)?;
        let target = self.target.clone().expect("render target");
        unsafe {
            target.DrawLine(
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x, y: y0 },
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x, y: y1 },
                &brush,
                1.0,
                None,
            );
        }
        Ok(())
    }

    pub fn push_clip(&mut self, rect: D2D_RECT_F) -> Result<()> {
        let target = self.target.clone().expect("render target");
        unsafe { target.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        self.clips += 1;
        Ok(())
    }

    pub fn pop_clip(&mut self) {
        if self.clips > 0 {
            if let Some(target) = &self.target {
                unsafe { target.PopAxisAlignedClip() };
            }
            self.clips -= 1;
        }
    }

    /// Applies a scroll offset to everything drawn until `reset_transform`.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let target = self.target.clone().expect("render target");
        let matrix = Matrix3x2::translation(dx, dy);
        self.transforms.push(matrix);
        unsafe { target.SetTransform(&matrix) };
    }

    pub fn reset_transform(&mut self) {
        if self.transforms.pop().is_some() {
            let matrix = self
                .transforms
                .last()
                .copied()
                .unwrap_or_else(Matrix3x2::identity);
            let target = self.target.clone().expect("render target");
            unsafe { target.SetTransform(&matrix) };
        }
    }

    pub fn draw_text(
        &mut self,
        text: &str,
        rect: D2D_RECT_F,
        rgb: u32,
        font: Font,
        align: TextAlign,
        clip: bool,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let format = self.format(font)?;
        unsafe {
            format.SetTextAlignment(match align {
                TextAlign::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
                TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            })?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let width = (rect.right - rect.left).max(1.0);
        let height = (rect.bottom - rect.top).max(1.0);
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&wide, &format, width, height)?
        };
        let brush = self.brush(rgb, 1.0)?;
        let target = self.target.clone().expect("render target");
        unsafe {
            target.DrawTextLayout(
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                    x: rect.left,
                    y: rect.top,
                },
                &layout,
                &brush,
                if clip {
                    D2D1_DRAW_TEXT_OPTIONS_CLIP
                } else {
                    D2D1_DRAW_TEXT_OPTIONS_NONE
                },
            );
        }
        Ok(())
    }

    pub fn draw_text_ellipsis(
        &mut self,
        text: &str,
        rect: D2D_RECT_F,
        rgb: u32,
        font: Font,
        align: TextAlign,
    ) -> Result<()> {
        // Direct2D's simple DrawText has no trimming; measure and truncate.
        let width = rect.right - rect.left;
        let mut label = text.to_owned();
        if self.text_width(&label, font)? > width {
            while !label.is_empty() && self.text_width(&label, font)? > width - 10.0 {
                label.pop();
            }
            label.push('…');
        }
        self.draw_text(&label, rect, rgb, font, align, false)
    }

    fn layout(&mut self, text: &str, font: Font, max_width: f32) -> Result<IDWriteTextLayout> {
        let format = self.format(font)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.dwrite
                .CreateTextLayout(&wide, &format, max_width.max(1.0), font.size() * 2.0)
        }
    }

    pub fn text_width(&mut self, text: &str, font: Font) -> Result<f32> {
        if text.is_empty() {
            return Ok(0.0);
        }
        let layout = self.layout(text, font, 10_000.0)?;
        let mut metrics = Default::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok(metrics.widthIncludingTrailingWhitespace)
    }

    /// Caret x offset for a character index inside `text`.
    pub fn caret_x(&mut self, text: &str, index: usize, font: Font) -> Result<f32> {
        if text.is_empty() {
            return Ok(0.0);
        }
        let layout = self.layout(text, font, 10_000.0)?;
        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        let mut metrics = Default::default();
        unsafe {
            layout.HitTestTextPosition(
                index.min(text.chars().count()) as u32,
                windows::Win32::Foundation::BOOL(0),
                &mut x,
                &mut y,
                &mut metrics,
            )?;
        }
        Ok(x)
    }

    /// Character index for a click at `x` inside `text`.
    pub fn caret_index(&mut self, text: &str, x: f32, font: Font) -> Result<usize> {
        if text.is_empty() {
            return Ok(0);
        }
        let layout = self.layout(text, font, 10_000.0)?;
        let mut metrics = Default::default();
        let mut trailing = windows::Win32::Foundation::BOOL(0);
        let mut inside = windows::Win32::Foundation::BOOL(0);
        unsafe {
            layout.HitTestPoint(
                x,
                font.size() * 0.5,
                &mut trailing,
                &mut inside,
                &mut metrics,
            )?;
        }
        Ok(metrics.textPosition as usize)
    }

    /// Line height for the editor and list rows.
    pub fn line_height(&mut self, font: Font) -> f32 {
        font.size() * 1.5
    }
}

/// Registers the embedded Satoshi font with a private DirectWrite collection.
///
/// The ITF Free Font License allows embedding the font in applications, so the
/// bytes are compiled in by `build.rs`; without them DirectWrite falls back to
/// the system family.
fn load_satoshi(dwrite: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    use windows::Win32::Graphics::DirectWrite::IDWriteFactory5;

    let bytes = crate::satoshi::SATOSHI?;
    unsafe {
        let factory: IDWriteFactory5 = dwrite.cast().ok()?;
        let loader = factory.CreateInMemoryFontFileLoader().ok()?;
        factory.RegisterFontFileLoader(&loader).ok()?;
        let file = loader
            .CreateInMemoryFontFileReference(
                dwrite,
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                None,
            )
            .ok()?;
        let builder = factory.CreateFontSetBuilder().ok()?;
        builder.AddFontFile(&file).ok()?;
        let set = builder.CreateFontSet().ok()?;
        let collection = factory.CreateFontCollectionFromFontSet(&set).ok()?;
        collection.cast().ok()
    }
}

/// Convenience constructor for rectangles.
pub fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

pub fn width(rect: D2D_RECT_F) -> f32 {
    rect.right - rect.left
}

pub fn height(rect: D2D_RECT_F) -> f32 {
    rect.bottom - rect.top
}

pub fn contains(rect: D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}
