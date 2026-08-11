//! Ícone do app desenhado em código (RGBA) — sem crate de imagem nem asset externo.
//! O quê: a marca schematize (quadrado azul arredondado + grafo de 3 nós branco).
//! Onde: usado pela GUI (ícone da janela/taskbar). O mesmo visual é gerado em PNG
//! (packaging/schematize.png) para o lançador (.desktop) e as notificações.

const BLUE: [u8; 3] = [47, 84, 255]; // #2f54ff — accent da marca
const WHITE: [u8; 3] = [245, 247, 250];

/// Gera o ícone em RGBA (com alpha) no tamanho `n` (px). Retorna (bytes, w, h).
pub fn rgba(n: u32) -> (Vec<u8>, u32, u32) {
    let nf = n as f32;
    let mut buf = vec![0u8; (n * n * 4) as usize];

    // Cantos arredondados: raio ~22% do lado.
    let radius = nf * 0.22;
    // Nós do grafo (fração do lado) e espessura das arestas.
    let nodes = [(0.31, 0.33), (0.70, 0.50), (0.35, 0.71)];
    let edges = [(0usize, 1usize), (1usize, 2usize)];
    let node_r = nf * 0.078;
    let edge_w = nf * 0.052;

    for y in 0..n {
        for x in 0..n {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let idx = ((y * n + x) * 4) as usize;

            // Fundo: dentro do quadrado arredondado?
            if !inside_rounded(px, py, nf, radius) {
                continue; // transparente
            }
            let mut color = BLUE;

            // Marca branca por cima: arestas (linhas grossas) e nós (círculos).
            let mut white = false;
            for &(a, b) in &edges {
                let p1 = (nodes[a].0 * nf, nodes[a].1 * nf);
                let p2 = (nodes[b].0 * nf, nodes[b].1 * nf);
                if dist_to_segment(px, py, p1, p2) <= edge_w * 0.5 {
                    white = true;
                    break;
                }
            }
            if !white {
                for &(cx, cy) in &nodes {
                    let dx = px - cx * nf;
                    let dy = py - cy * nf;
                    if dx * dx + dy * dy <= node_r * node_r {
                        white = true;
                        break;
                    }
                }
            }
            if white {
                color = WHITE;
            }

            buf[idx] = color[0];
            buf[idx + 1] = color[1];
            buf[idx + 2] = color[2];
            buf[idx + 3] = 255;
        }
    }
    (buf, n, n)
}

/// Ponto dentro de um quadrado (0..n) com cantos de raio `r`.
fn inside_rounded(px: f32, py: f32, n: f32, r: f32) -> bool {
    if px < 0.0 || py < 0.0 || px > n || py > n {
        return false;
    }
    let cx = px.clamp(r, n - r);
    let cy = py.clamp(r, n - r);
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= r * r
}

/// Distância de um ponto ao segmento p1-p2.
fn dist_to_segment(px: f32, py: f32, p1: (f32, f32), p2: (f32, f32)) -> f32 {
    let (x1, y1) = p1;
    let (x2, y2) = p2;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len2).clamp(0.0, 1.0);
    let projx = x1 + t * dx;
    let projy = y1 + t * dy;
    ((px - projx).powi(2) + (py - projy).powi(2)).sqrt()
}
