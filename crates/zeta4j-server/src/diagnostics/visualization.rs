//! # 진단 시각화 모듈
//!
//! 메트릭과 진단 데이터를 ASCII 차트와 대시보드 형태로 시각화합니다.
//!
//! ## 주요 기능
//!
//! - **ASCII 차트**: 터미널에서 시계열 데이터 시각화
//! - **대시보드 레이아웃**: 여러 메트릭을 한눈에 보는 대시보드
//! - **스파크라인**: 콤팩트한 추세 표시
//! - **상태 표시기**: 건강 상태 시각적 표현
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_server::diagnostics::visualization::{AsciiChart, Dashboard};
//!
//! // 시계열 차트
//! let chart = AsciiChart::new("TPS History", 40, 10);
//! let data = vec![100.0, 120.0, 150.0, 130.0, 140.0];
//! println!("{}", chart.render(&data));
//!
//! // 대시보드
//! let dashboard = Dashboard::new()
//!     .add_metric("TPS", 150.0, Some(100.0), Some(200.0))
//!     .add_metric("Memory", 2048.0, Some(0.0), Some(4096.0));
//! println!("{}", dashboard);
//! ```

use std::fmt;

/// ASCII 차트
///
/// 시계열 데이터를 ASCII 그래프로 렌더링합니다.
#[derive(Debug, Clone)]
pub struct AsciiChart {
    /// 차트 제목
    title: String,
    /// 차트 너비 (문자)
    width: usize,
    /// 차트 높이 (라인)
    height: usize,
    /// Y축 최소값 (자동 계산시 None)
    y_min: Option<f64>,
    /// Y축 최대값 (자동 계산시 None)
    y_max: Option<f64>,
}

impl AsciiChart {
    /// 새 차트 생성
    pub fn new(title: impl Into<String>, width: usize, height: usize) -> Self {
        Self {
            title: title.into(),
            width: width.max(10),
            height: height.max(3),
            y_min: None,
            y_max: None,
        }
    }

    /// Y축 범위 설정
    pub fn with_y_range(mut self, min: f64, max: f64) -> Self {
        self.y_min = Some(min);
        self.y_max = Some(max);
        self
    }

    /// 데이터 렌더링
    pub fn render(&self, data: &[f64]) -> String {
        if data.is_empty() {
            return format!("{}\n(No data)", self.title);
        }

        // Y축 범위 계산
        let y_min = self
            .y_min
            .unwrap_or_else(|| data.iter().copied().fold(f64::INFINITY, f64::min));
        let y_max = self
            .y_max
            .unwrap_or_else(|| data.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        let y_range = if y_max > y_min { y_max - y_min } else { 1.0 };

        // 데이터 샘플링 (너비에 맞게)
        let sampled = self.sample_data(data);

        // 그래프 그리기
        let mut lines = Vec::new();

        // 제목
        lines.push(format!(
            "┌─ {} {}",
            self.title,
            "─".repeat(self.width.saturating_sub(self.title.len() + 4))
        ));

        // Y축 레이블과 그래프
        for row in 0..self.height {
            let y_value = y_max - (row as f64 / (self.height - 1) as f64) * y_range;
            let label = format!("{:8.1}", y_value);

            let mut line_chars = Vec::new();
            for (i, &value) in sampled.iter().enumerate() {
                let normalized = ((value - y_min) / y_range).clamp(0.0, 1.0);
                let value_row = ((1.0 - normalized) * (self.height - 1) as f64).round() as usize;

                let ch = if value_row == row {
                    if i == 0 || i == sampled.len() - 1 {
                        '◆'
                    } else {
                        '●'
                    }
                } else {
                    ' '
                };
                line_chars.push(ch);
            }

            let line_str: String = line_chars.into_iter().collect();
            lines.push(format!("│{} │{}", label, line_str));
        }

        // X축
        lines.push(format!("└{}┴{}", "─".repeat(9), "─".repeat(self.width)));

        lines.join("\n")
    }

    /// 데이터 샘플링
    fn sample_data(&self, data: &[f64]) -> Vec<f64> {
        if data.len() <= self.width {
            return data.to_vec();
        }

        let step = data.len() as f64 / self.width as f64;
        (0..self.width)
            .map(|i| {
                let idx = (i as f64 * step) as usize;
                data[idx.min(data.len() - 1)]
            })
            .collect()
    }
}

/// 스파크라인
///
/// 콤팩트한 시계열 추세 표시
#[derive(Debug, Clone)]
pub struct Sparkline {
    /// 데이터
    data: Vec<f64>,
    /// 최대 길이
    max_len: usize,
}

impl Sparkline {
    /// 새 스파크라인 생성
    pub fn new(max_len: usize) -> Self {
        Self {
            data: Vec::new(),
            max_len,
        }
    }

    /// 데이터로부터 생성
    pub fn from_data(data: Vec<f64>, max_len: usize) -> Self {
        let mut s = Self::new(max_len);
        for v in data {
            s.push(v);
        }
        s
    }

    /// 데이터 포인트 추가
    pub fn push(&mut self, value: f64) {
        self.data.push(value);
        if self.data.len() > self.max_len {
            self.data.remove(0);
        }
    }

    /// 렌더링
    pub fn render(&self) -> String {
        const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        if self.data.is_empty() {
            return String::new();
        }

        let min = self.data.iter().copied().fold(f64::INFINITY, f64::min);
        let max = self.data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let range = if max > min { max - min } else { 1.0 };

        self.data
            .iter()
            .map(|&v| {
                let normalized = ((v - min) / range).clamp(0.0, 1.0);
                let idx = (normalized * 7.0).round() as usize;
                BLOCKS[idx.min(7)]
            })
            .collect()
    }
}

impl fmt::Display for Sparkline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// 진행률 막대
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// 레이블
    label: String,
    /// 현재 값
    value: f64,
    /// 최대 값
    max_value: f64,
    /// 바 너비
    width: usize,
}

impl ProgressBar {
    /// 새 진행률 막대 생성
    pub fn new(label: impl Into<String>, value: f64, max_value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            max_value,
            width: 30,
        }
    }

    /// 너비 설정
    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// 렌더링
    pub fn render(&self) -> String {
        let percentage = if self.max_value > 0.0 {
            (self.value / self.max_value * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let filled = (percentage / 100.0 * self.width as f64).round() as usize;
        let empty = self.width.saturating_sub(filled);

        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        format!("{:12} [{bar}] {:5.1}%", self.label, percentage)
    }
}

impl fmt::Display for ProgressBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// 상태 표시기
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIndicator {
    /// 정상
    Ok,
    /// 경고
    Warning,
    /// 위험
    Critical,
    /// 알 수 없음
    Unknown,
}

impl StatusIndicator {
    /// 값 기반 상태 결정
    pub fn from_threshold(value: f64, warning: f64, critical: f64) -> Self {
        if value >= critical {
            StatusIndicator::Critical
        } else if value >= warning {
            StatusIndicator::Warning
        } else {
            StatusIndicator::Ok
        }
    }

    /// 아이콘 반환
    pub fn icon(&self) -> &'static str {
        match self {
            StatusIndicator::Ok => "✓",
            StatusIndicator::Warning => "⚠",
            StatusIndicator::Critical => "✗",
            StatusIndicator::Unknown => "?",
        }
    }

    /// 색상 이름 반환
    pub fn color_name(&self) -> &'static str {
        match self {
            StatusIndicator::Ok => "green",
            StatusIndicator::Warning => "yellow",
            StatusIndicator::Critical => "red",
            StatusIndicator::Unknown => "gray",
        }
    }
}

impl fmt::Display for StatusIndicator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.icon())
    }
}

/// 대시보드 메트릭 항목
#[derive(Debug, Clone)]
pub struct MetricWidget {
    /// 메트릭 이름
    pub name: String,
    /// 현재 값
    pub value: f64,
    /// 단위
    pub unit: String,
    /// 최소값 (진행률 계산용)
    pub min: Option<f64>,
    /// 최대값 (진행률 계산용)
    pub max: Option<f64>,
    /// 경고 임계값
    pub warning_threshold: Option<f64>,
    /// 위험 임계값
    pub critical_threshold: Option<f64>,
    /// 히스토리 (스파크라인용)
    pub history: Vec<f64>,
}

impl MetricWidget {
    /// 새 메트릭 위젯 생성
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: String::new(),
            min: None,
            max: None,
            warning_threshold: None,
            critical_threshold: None,
            history: Vec::new(),
        }
    }

    /// 단위 설정
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// 범위 설정
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// 임계값 설정
    pub fn with_thresholds(mut self, warning: f64, critical: f64) -> Self {
        self.warning_threshold = Some(warning);
        self.critical_threshold = Some(critical);
        self
    }

    /// 히스토리 설정
    pub fn with_history(mut self, history: Vec<f64>) -> Self {
        self.history = history;
        self
    }

    /// 상태 가져오기
    pub fn status(&self) -> StatusIndicator {
        match (self.warning_threshold, self.critical_threshold) {
            (Some(w), Some(c)) => StatusIndicator::from_threshold(self.value, w, c),
            _ => StatusIndicator::Unknown,
        }
    }

    /// 렌더링
    pub fn render(&self) -> String {
        let status = self.status();
        let sparkline = if !self.history.is_empty() {
            format!(" {}", Sparkline::from_data(self.history.clone(), 10))
        } else {
            String::new()
        };

        let progress = if let (Some(min), Some(max)) = (self.min, self.max) {
            let pct = ((self.value - min) / (max - min) * 100.0).clamp(0.0, 100.0);
            format!(" ({:.0}%)", pct)
        } else {
            String::new()
        };

        format!(
            "{} {:12}: {:>10.2} {}{}{}",
            status, self.name, self.value, self.unit, progress, sparkline
        )
    }
}

impl fmt::Display for MetricWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// 대시보드
///
/// 여러 메트릭을 종합적으로 표시
#[derive(Debug, Clone)]
pub struct Dashboard {
    /// 대시보드 제목
    title: String,
    /// 메트릭 위젯들
    widgets: Vec<MetricWidget>,
    /// 섹션 구분
    sections: Vec<(String, Vec<usize>)>,
}

impl Dashboard {
    /// 새 대시보드 생성
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            widgets: Vec::new(),
            sections: Vec::new(),
        }
    }

    /// 메트릭 추가
    pub fn add_metric(mut self, widget: MetricWidget) -> Self {
        self.widgets.push(widget);
        self
    }

    /// 간단한 메트릭 추가
    pub fn add_simple_metric(
        mut self,
        name: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
    ) -> Self {
        self.widgets
            .push(MetricWidget::new(name, value).with_unit(unit));
        self
    }

    /// 섹션 시작
    pub fn start_section(mut self, name: impl Into<String>) -> Self {
        let start_idx = self.widgets.len();
        self.sections.push((name.into(), vec![start_idx]));
        self
    }

    /// 전체 상태 계산
    pub fn overall_status(&self) -> StatusIndicator {
        let statuses: Vec<_> = self.widgets.iter().map(|w| w.status()).collect();

        if statuses.contains(&StatusIndicator::Critical) {
            StatusIndicator::Critical
        } else if statuses.contains(&StatusIndicator::Warning) {
            StatusIndicator::Warning
        } else if statuses.iter().all(|s| *s == StatusIndicator::Ok) {
            StatusIndicator::Ok
        } else {
            StatusIndicator::Unknown
        }
    }

    /// 렌더링
    pub fn render(&self) -> String {
        let overall = self.overall_status();
        let title_line = format!("╔═══ {} ═══╗", self.title);
        let width = title_line.chars().count().max(50);

        let mut lines = Vec::new();

        // 상단 테두리
        lines.push(format!("╔{}╗", "═".repeat(width - 2)));
        lines.push(format!(
            "║ {} {} {:>width$}║",
            overall.icon(),
            self.title,
            "",
            width = width - self.title.len() - 6
        ));
        lines.push(format!("╠{}╣", "═".repeat(width - 2)));

        // 메트릭들
        for widget in &self.widgets {
            let content = widget.render();
            let padding = width.saturating_sub(content.chars().count() + 4);
            lines.push(format!("║ {}{}║", content, " ".repeat(padding)));
        }

        // 하단 테두리
        lines.push(format!("╚{}╝", "═".repeat(width - 2)));

        lines.join("\n")
    }
}

impl fmt::Display for Dashboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// 테이블 렌더러
///
/// 데이터를 ASCII 테이블로 표시
#[derive(Debug, Clone)]
pub struct Table {
    /// 헤더
    headers: Vec<String>,
    /// 행 데이터
    rows: Vec<Vec<String>>,
    /// 열 너비 (자동 계산)
    col_widths: Vec<usize>,
}

impl Table {
    /// 새 테이블 생성
    pub fn new(headers: Vec<impl Into<String>>) -> Self {
        let headers: Vec<String> = headers.into_iter().map(|h| h.into()).collect();
        let col_widths = headers.iter().map(|h| h.len()).collect();
        Self {
            headers,
            rows: Vec::new(),
            col_widths,
        }
    }

    /// 행 추가
    pub fn add_row(mut self, row: Vec<impl Into<String>>) -> Self {
        let row: Vec<String> = row.into_iter().map(|c| c.into()).collect();

        // 열 너비 업데이트
        for (i, cell) in row.iter().enumerate() {
            if i < self.col_widths.len() {
                self.col_widths[i] = self.col_widths[i].max(cell.len());
            }
        }

        self.rows.push(row);
        self
    }

    /// 렌더링
    pub fn render(&self) -> String {
        let mut lines = Vec::new();

        // 구분선 생성
        let separator: String = self
            .col_widths
            .iter()
            .map(|&w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┼");

        // 헤더
        let header_line: String = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!(" {:width$} ", h, width = self.col_widths[i]))
            .collect::<Vec<_>>()
            .join("│");

        lines.push(format!("┌{}┐", separator.replace("┼", "┬")));
        lines.push(format!("│{}│", header_line));
        lines.push(format!("├{}┤", separator));

        // 데이터 행
        for row in &self.rows {
            let row_line: String = row
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let width = self.col_widths.get(i).copied().unwrap_or(10);
                    format!(" {:width$} ", c, width = width)
                })
                .collect::<Vec<_>>()
                .join("│");
            lines.push(format!("│{}│", row_line));
        }

        lines.push(format!("└{}┘", separator.replace("┼", "┴")));

        lines.join("\n")
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}
