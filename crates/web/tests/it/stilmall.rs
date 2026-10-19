//! Stilmallen `style/maya.css` som cargo-leptos lägger i `target/site/pkg/maya.css`.

const STILMALL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/style/maya.css"));

/// Raderna (1-baserade) där ett block öppnas som aldrig stängs, och de där ett block stängs
/// som aldrig öppnats. Kommentarer räknas inte.
fn obalanserade_block(css: &str) -> (Vec<usize>, Vec<usize>) {
    let mut öppna = Vec::new();
    let mut överskott = Vec::new();
    let mut rad = 1;
    let mut i = 0;
    let tecken: Vec<char> = css.chars().collect();
    while i < tecken.len() {
        match tecken[i] {
            '/' if tecken.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < tecken.len() && !(tecken[i] == '*' && tecken.get(i + 1) == Some(&'/')) {
                    if tecken[i] == '\n' {
                        rad += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            '\n' => rad += 1,
            '{' => öppna.push(rad),
            '}' if öppna.pop().is_none() => överskott.push(rad),
            _ => {}
        }
        i += 1;
    }
    (öppna, överskott)
}

#[test]
fn varje_block_i_stilmallen_stangs() {
    // En regel som inte stängs gör att webbläsaren läser resten av filen som en del av den,
    // och då gäller inga av reglerna efter den.
    assert_eq!(obalanserade_block(STILMALL), (vec![], vec![]));
}
