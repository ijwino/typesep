pub const TEX_PT_PER_TYPST_PT: f64 = 72.27 / 72.0;

pub fn typst_pt(tex_pt: f64) -> String {
    format!(
        "{}pt",
        typesep_style::units::fmt_pt(tex_pt / TEX_PT_PER_TYPST_PT)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typst_points_are_tex_points_converted() {
        assert_eq!(typst_pt(15.0), "14.944pt");
        assert_eq!(typst_pt(5.5 * 72.27), "396pt");
        assert_eq!(typst_pt(72.27), "72pt");
    }
}
