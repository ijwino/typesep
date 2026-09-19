const BASE: [(&str, &str); 19] = [
    ("red", "#FF0000"),
    ("green", "#00FF00"),
    ("blue", "#0000FF"),
    ("cyan", "#00FFFF"),
    ("magenta", "#FF00FF"),
    ("yellow", "#FFFF00"),
    ("black", "#000000"),
    ("gray", "#808080"),
    ("white", "#FFFFFF"),
    ("darkgray", "#404040"),
    ("lightgray", "#BFBFBF"),
    ("brown", "#BF8040"),
    ("lime", "#BFFF00"),
    ("olive", "#808000"),
    ("orange", "#FF8000"),
    ("pink", "#FFBFBF"),
    ("purple", "#BF0040"),
    ("teal", "#008080"),
    ("violet", "#8F00FF"),
];

const NAMED: [(&str, &str); 152] = [
    ("aliceblue", "#F0F8FF"),
    ("antiquewhite", "#FAEBD7"),
    ("aqua", "#00FFFF"),
    ("aquamarine", "#7FFFD4"),
    ("azure", "#F0FFFF"),
    ("beige", "#F5F5DC"),
    ("bisque", "#FFE4C4"),
    ("black", "#000000"),
    ("blanchedalmond", "#FFEBCD"),
    ("blue", "#0000FF"),
    ("blueviolet", "#8A2BE2"),
    ("brown", "#A52A2A"),
    ("burlywood", "#DEB887"),
    ("cadetblue", "#5F9EA0"),
    ("chartreuse", "#7FFF00"),
    ("chocolate", "#D2691E"),
    ("coral", "#FF7F50"),
    ("cornflowerblue", "#6495ED"),
    ("cornsilk", "#FFF8DC"),
    ("crimson", "#DC143C"),
    ("cyan", "#00FFFF"),
    ("darkblue", "#00008B"),
    ("darkcyan", "#008B8B"),
    ("darkgoldenrod", "#B8860B"),
    ("darkgray", "#A9A9A9"),
    ("darkgreen", "#006400"),
    ("darkgrey", "#A9A9A9"),
    ("darkkhaki", "#BDB76B"),
    ("darkmagenta", "#8B008B"),
    ("darkolivegreen", "#556B2F"),
    ("darkorange", "#FF8C00"),
    ("darkorchid", "#9932CC"),
    ("darkred", "#8B0000"),
    ("darksalmon", "#E9967A"),
    ("darkseagreen", "#8FBC8F"),
    ("darkslateblue", "#483D8B"),
    ("darkslategray", "#2F4F4F"),
    ("darkslategrey", "#2F4F4F"),
    ("darkturquoise", "#00CED1"),
    ("darkviolet", "#9400D3"),
    ("deeppink", "#FF1493"),
    ("deepskyblue", "#00BFFF"),
    ("dimgray", "#696969"),
    ("dimgrey", "#696969"),
    ("dodgerblue", "#1E90FF"),
    ("firebrick", "#B22222"),
    ("floralwhite", "#FFFAF0"),
    ("forestgreen", "#228B22"),
    ("fuchsia", "#FF00FF"),
    ("gainsboro", "#DCDCDC"),
    ("ghostwhite", "#F8F8FF"),
    ("gold", "#FFD700"),
    ("goldenrod", "#DAA520"),
    ("gray", "#808080"),
    ("green", "#008000"),
    ("greenyellow", "#ADFF2F"),
    ("grey", "#808080"),
    ("honeydew", "#F0FFF0"),
    ("hotpink", "#FF69B4"),
    ("indianred", "#CD5C5C"),
    ("indigo", "#4B0082"),
    ("ivory", "#FFFFF0"),
    ("khaki", "#F0E68C"),
    ("lavender", "#E6E6FA"),
    ("lavenderblush", "#FFF0F5"),
    ("lawngreen", "#7CFC00"),
    ("lemonchiffon", "#FFFACD"),
    ("lightblue", "#ADD8E6"),
    ("lightcoral", "#F08080"),
    ("lightcyan", "#E0FFFF"),
    ("lightgoldenrod", "#EEDD82"),
    ("lightgoldenrodyellow", "#FAFAD2"),
    ("lightgray", "#D3D3D3"),
    ("lightgreen", "#90EE90"),
    ("lightgrey", "#D3D3D3"),
    ("lightpink", "#FFB6C1"),
    ("lightsalmon", "#FFA07A"),
    ("lightseagreen", "#20B2AA"),
    ("lightskyblue", "#87CEFA"),
    ("lightslateblue", "#8470FF"),
    ("lightslategray", "#778899"),
    ("lightslategrey", "#778899"),
    ("lightsteelblue", "#B0C4DE"),
    ("lightyellow", "#FFFFE0"),
    ("lime", "#00FF00"),
    ("limegreen", "#32CD32"),
    ("linen", "#FAF0E6"),
    ("magenta", "#FF00FF"),
    ("maroon", "#800000"),
    ("mediumaquamarine", "#66CDAA"),
    ("mediumblue", "#0000CD"),
    ("mediumorchid", "#BA55D3"),
    ("mediumpurple", "#9370DB"),
    ("mediumseagreen", "#3CB371"),
    ("mediumslateblue", "#7B68EE"),
    ("mediumspringgreen", "#00FA9A"),
    ("mediumturquoise", "#48D1CC"),
    ("mediumvioletred", "#C71585"),
    ("midnightblue", "#191970"),
    ("mintcream", "#F5FFFA"),
    ("mistyrose", "#FFE4E1"),
    ("moccasin", "#FFE4B5"),
    ("navajowhite", "#FFDEAD"),
    ("navy", "#000080"),
    ("navyblue", "#000080"),
    ("oldlace", "#FDF5E6"),
    ("olive", "#808000"),
    ("olivedrab", "#6B8E23"),
    ("orange", "#FFA500"),
    ("orangered", "#FF4500"),
    ("orchid", "#DA70D6"),
    ("palegoldenrod", "#EEE8AA"),
    ("palegreen", "#98FB98"),
    ("paleturquoise", "#AFEEEE"),
    ("palevioletred", "#DB7093"),
    ("papayawhip", "#FFEFD5"),
    ("peachpuff", "#FFDAB9"),
    ("peru", "#CD853F"),
    ("pink", "#FFC0CB"),
    ("plum", "#DDA0DD"),
    ("powderblue", "#B0E0E6"),
    ("purple", "#800080"),
    ("rebeccapurple", "#663399"),
    ("red", "#FF0000"),
    ("rosybrown", "#BC8F8F"),
    ("royalblue", "#4169E1"),
    ("saddlebrown", "#8B4513"),
    ("salmon", "#FA8072"),
    ("sandybrown", "#F4A460"),
    ("seagreen", "#2E8B57"),
    ("seashell", "#FFF5EE"),
    ("sienna", "#A0522D"),
    ("silver", "#C0C0C0"),
    ("skyblue", "#87CEEB"),
    ("slateblue", "#6A5ACD"),
    ("slategray", "#708090"),
    ("slategrey", "#708090"),
    ("snow", "#FFFAFA"),
    ("springgreen", "#00FF7F"),
    ("steelblue", "#4682B4"),
    ("tan", "#D2B48C"),
    ("teal", "#008080"),
    ("thistle", "#D8BFD8"),
    ("tomato", "#FF6347"),
    ("turquoise", "#40E0D0"),
    ("violet", "#EE82EE"),
    ("violetred", "#D02090"),
    ("wheat", "#F5DEB3"),
    ("white", "#FFFFFF"),
    ("whitesmoke", "#F5F5F5"),
    ("yellow", "#FFFF00"),
    ("yellowgreen", "#9ACD32"),
];

pub fn resolve(value: &str) -> Option<String> {
    let name = value.trim();
    if let Some(hex) = name.strip_prefix('#') {
        return Some(format!("#{}", hex.to_uppercase()));
    }
    if let Some((_, hex)) = BASE.iter().find(|(n, _)| *n == name) {
        return Some((*hex).to_owned());
    }
    let lower = name.to_lowercase();
    NAMED
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, hex)| (*hex).to_owned())
}

pub fn color_expr(value: &str, warn: &mut dyn FnMut(&str)) -> String {
    match resolve(value) {
        Some(hex) => format!("rgb(\"{hex}\")"),
        None => {
            warn(&format!(
                "unknown color \"{value}\" in the style; falling back to black"
            ));
            "rgb(\"#000000\")".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_the_way_xcolor_defined_them() {
        assert_eq!(resolve("black").as_deref(), Some("#000000"));
        assert_eq!(
            resolve("violet").as_deref(),
            Some("#8F00FF"),
            "xcolor's base violet, not the CSS one"
        );
        assert_eq!(
            resolve("Violet").as_deref(),
            Some("#EE82EE"),
            "svgnames is matched ignoring case"
        );
        assert_eq!(
            resolve("green").as_deref(),
            Some("#00FF00"),
            "xcolor's base green"
        );
        assert_eq!(resolve("ForestGreen").as_deref(), Some("#228B22"));
        assert_eq!(resolve("#334455").as_deref(), Some("#334455"));
        assert_eq!(resolve("#aabbcc").as_deref(), Some("#AABBCC"));
        assert_eq!(resolve("nosuchcolor"), None);
    }

    #[test]
    fn an_unknown_color_warns_and_goes_black() {
        let mut said = Vec::new();
        let expr = color_expr("nosuchcolor", &mut |m| said.push(m.to_owned()));
        assert_eq!(expr, "rgb(\"#000000\")");
        assert_eq!(
            said,
            ["unknown color \"nosuchcolor\" in the style; falling back to black"]
        );
        assert_eq!(color_expr("#112233", &mut |_| {}), "rgb(\"#112233\")");
    }
}
