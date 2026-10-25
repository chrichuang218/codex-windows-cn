//! Issue #44：WebView2 会把 Windows「辅助功能 → 文本大小」乘进页面缩放，
//! 122% 时原 820×580 的窗口只剩约 674×476 CSS px，页面会落入窄屏布局。
//! 这里保留系统文本放大，改为按比例放大初始窗口，让页面拿回设计尺寸的布局空间。

/// 宽高对；单位由所在字段说明（逻辑像素或物理像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSizeInput {
    /// 配置中的内容区尺寸，逻辑像素。
    pub base: Size,
    /// 配置中的最小内容区尺寸，逻辑像素。
    pub min: Size,
    /// 系统文本缩放，公开 API 契约范围 1.0–2.25。
    pub text_scale: f64,
    /// 窗口所在显示器的 DPI 缩放。
    pub dpi_scale: f64,
    /// 显示器工作区，物理像素。
    pub work_area: Size,
    /// 非客户区开销（外框 − 内容区），物理像素。
    pub frame: Size,
}

/// 补偿后的内容区尺寸和最小尺寸，逻辑像素。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSizePlan {
    pub size: Size,
    pub min: Size,
}

const TEXT_SCALE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=2.25;

/// 文本缩放为 100% 时返回 `Ok(None)`，保持配置窗口不变。
///
/// 上限按「工作区 − 非客户区开销」在物理像素下计算，再换算成逻辑像素；
/// 屏幕放不下时，最小尺寸跟着降到上限以内，否则系统最小尺寸约束会把窗口撑出工作区。
pub fn compensated_window_size(input: &WindowSizeInput) -> Result<Option<WindowSizePlan>, String> {
    if !TEXT_SCALE_RANGE.contains(&input.text_scale) {
        return Err(format!("系统文本缩放超出 1.0–2.25：{}", input.text_scale));
    }
    if !(input.dpi_scale.is_finite() && input.dpi_scale > 0.0) {
        return Err(format!("显示器 DPI 缩放无效：{}", input.dpi_scale));
    }
    if input.text_scale == 1.0 {
        return Ok(None);
    }

    let available = Size::new(
        ((input.work_area.width - input.frame.width) / input.dpi_scale).floor(),
        ((input.work_area.height - input.frame.height) / input.dpi_scale).floor(),
    );
    if available.width < 1.0 || available.height < 1.0 {
        return Err(format!(
            "工作区 {}×{} 扣除窗口边框 {}×{} 后没有可用内容区",
            input.work_area.width, input.work_area.height, input.frame.width, input.frame.height
        ));
    }

    let size = Size::new(
        (input.base.width * input.text_scale)
            .round()
            .min(available.width),
        (input.base.height * input.text_scale)
            .round()
            .min(available.height),
    );
    let min = Size::new(
        (input.min.width * input.text_scale).round().min(size.width),
        (input.min.height * input.text_scale)
            .round()
            .min(size.height),
    );
    Ok(Some(WindowSizePlan { size, min }))
}

/// 读取 Windows「辅助功能 → 文本大小」。WebView2 文档说明其 RasterizationScale
/// 由显示器 DPI 与这项用户文本缩放组合而成，官方示例也用 UISettings 读取它。
#[cfg(windows)]
pub fn system_text_scale() -> Result<f64, String> {
    windows::UI::ViewManagement::UISettings::new()
        .and_then(|settings| settings.TextScaleFactor())
        .map_err(|cause| format!("读取系统文本大小失败：{cause}"))
}

/// 非 Windows 平台的 WebView 不叠加 Windows 文本大小。
#[cfg(not(windows))]
pub fn system_text_scale() -> Result<f64, String> {
    Ok(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Size = Size {
        width: 820.0,
        height: 540.0,
    };
    const MIN: Size = Size {
        width: 720.0,
        height: 520.0,
    };
    // Win32 外框（含不可见调整边框）与内容区之差，100% / 150% DPI 下的实测量级。
    const FRAME_100: Size = Size {
        width: 16.0,
        height: 39.0,
    };
    const FRAME_150: Size = Size {
        width: 24.0,
        height: 58.0,
    };

    fn input(text_scale: f64, dpi_scale: f64, work_area: Size, frame: Size) -> WindowSizeInput {
        WindowSizeInput {
            base: BASE,
            min: MIN,
            text_scale,
            dpi_scale,
            work_area,
            frame,
        }
    }

    fn plan(input: WindowSizeInput) -> WindowSizePlan {
        compensated_window_size(&input)
            .expect("valid input")
            .expect("text scale above 100% needs compensation")
    }

    #[test]
    fn default_text_size_keeps_configured_window() {
        let result =
            compensated_window_size(&input(1.0, 1.0, Size::new(1920.0, 1032.0), FRAME_100));

        assert_eq!(result, Ok(None));
    }

    #[test]
    fn text_size_122_scales_window_and_minimum() {
        let plan = plan(input(1.22, 1.0, Size::new(2560.0, 1392.0), FRAME_100));

        // 820×1.22=1000.4、540×1.22=658.8、720×1.22=878.4、520×1.22=634.4，四舍五入。
        assert_eq!(plan.size, Size::new(1000.0, 659.0));
        assert_eq!(plan.min, Size::new(878.0, 634.0));
    }

    #[test]
    fn text_size_225_scales_when_screen_is_large_enough() {
        let plan = plan(input(2.25, 1.0, Size::new(3840.0, 2112.0), FRAME_100));

        assert_eq!(plan.size, Size::new(1845.0, 1215.0));
        assert_eq!(plan.min, Size::new(1620.0, 1170.0));
    }

    #[test]
    fn only_height_is_clamped_after_subtracting_frame_in_physical_pixels() {
        // 1920×1080 @150%：可用内容区 (1920−24)/1.5=1264、(1032−58)/1.5≈649。
        let plan = plan(input(1.22, 1.5, Size::new(1920.0, 1032.0), FRAME_150));

        assert_eq!(plan.size, Size::new(1000.0, 649.0));
        assert_eq!(plan.min, Size::new(878.0, 634.0));
    }

    #[test]
    fn only_width_is_clamped_when_screen_is_narrow() {
        // 可用内容区 1084×1361；1.5 倍后宽 1230 放不下、高 810 放得下。
        let plan = plan(input(1.5, 1.0, Size::new(1100.0, 1400.0), FRAME_100));

        assert_eq!(plan.size, Size::new(1084.0, 810.0));
        assert_eq!(plan.min, Size::new(1080.0, 780.0));
    }

    #[test]
    fn minimum_drops_below_configured_minimum_when_work_area_is_too_small() {
        // 1280×720 @150%：可用内容区约 837×409，比配置最小值 720×520 的高度还小。
        let plan = plan(input(1.22, 1.5, Size::new(1280.0, 672.0), FRAME_150));

        assert_eq!(plan.size, Size::new(837.0, 409.0));
        assert_eq!(plan.min, Size::new(837.0, 409.0));
        assert!(plan.min.height < MIN.height);
    }

    #[test]
    fn planned_size_fits_work_area_after_physical_rounding() {
        // tao 把逻辑尺寸乘 DPI 后四舍五入成物理像素；后三组会触发钳制，含恰好贴边的工作区。
        let cases = [
            (1.22, 1.25, Size::new(1920.0, 1032.0), Size::new(20.0, 48.0)),
            (2.25, 1.25, Size::new(1366.0, 728.0), Size::new(20.0, 48.0)),
            (1.22, 1.75, Size::new(1920.0, 1032.0), Size::new(28.0, 68.0)),
            (1.5, 1.75, Size::new(1501.0, 999.0), Size::new(28.0, 68.0)),
        ];

        for (text_scale, dpi_scale, work_area, frame) in cases {
            let plan = plan(input(text_scale, dpi_scale, work_area, frame));
            let axes = [
                (
                    plan.size.width,
                    plan.min.width,
                    work_area.width,
                    frame.width,
                ),
                (
                    plan.size.height,
                    plan.min.height,
                    work_area.height,
                    frame.height,
                ),
            ];
            for (size, min, work, frame) in axes {
                let context = format!("text {text_scale}, dpi {dpi_scale}, work {work}");
                assert!((size * dpi_scale).round() + frame <= work, "{context}");
                assert!(min <= size, "{context}");
            }
        }
    }

    #[test]
    fn rejects_text_scale_outside_public_api_range() {
        for text_scale in [0.0, 0.99, 2.26, f64::NAN, f64::INFINITY] {
            let result = compensated_window_size(&input(
                text_scale,
                1.0,
                Size::new(2560.0, 1392.0),
                FRAME_100,
            ));

            assert!(
                result.is_err(),
                "text scale {text_scale} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_invalid_dpi_scale() {
        for dpi_scale in [0.0, -1.0, f64::NAN] {
            let result = compensated_window_size(&input(
                1.22,
                dpi_scale,
                Size::new(2560.0, 1392.0),
                FRAME_100,
            ));

            assert!(result.is_err(), "dpi scale {dpi_scale} should be rejected");
        }
    }

    #[test]
    fn rejects_work_area_that_cannot_hold_any_client_area() {
        let result = compensated_window_size(&input(1.22, 1.0, Size::new(16.0, 39.0), FRAME_100));

        assert!(result.is_err());
    }
}
