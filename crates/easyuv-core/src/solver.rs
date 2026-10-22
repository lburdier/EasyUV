//! Matrices creuses et solveurs itératifs (CGLS pour moindres carrés, CG pour SPD).

/// Matrice creuse au format CSR.
pub struct Csr {
    pub rows: usize,
    pub cols: usize,
    pub row_start: Vec<usize>,
    pub col: Vec<u32>,
    pub val: Vec<f64>,
}

pub struct CsrBuilder {
    cols: usize,
    row_start: Vec<usize>,
    col: Vec<u32>,
    val: Vec<f64>,
}

impl CsrBuilder {
    pub fn new(cols: usize) -> Self {
        CsrBuilder { cols, row_start: vec![0], col: Vec::new(), val: Vec::new() }
    }
    pub fn push(&mut self, c: usize, v: f64) {
        self.col.push(c as u32);
        self.val.push(v);
    }
    pub fn end_row(&mut self) {
        self.row_start.push(self.col.len());
    }
    pub fn build(self) -> Csr {
        Csr { rows: self.row_start.len() - 1, cols: self.cols, row_start: self.row_start, col: self.col, val: self.val }
    }
}

impl Csr {
    pub fn mul(&self, x: &[f64], out: &mut [f64]) {
        for r in 0..self.rows {
            let mut s = 0.0;
            for k in self.row_start[r]..self.row_start[r + 1] {
                s += self.val[k] * x[self.col[k] as usize];
            }
            out[r] = s;
        }
    }
    pub fn mul_t(&self, y: &[f64], out: &mut [f64]) {
        out.iter_mut().for_each(|v| *v = 0.0);
        for r in 0..self.rows {
            let yr = y[r];
            if yr == 0.0 {
                continue;
            }
            for k in self.row_start[r]..self.row_start[r + 1] {
                out[self.col[k] as usize] += self.val[k] * yr;
            }
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Résout min ||A x - b|| par CGLS avec préconditionnement diagonal (normes de colonnes).
/// `x` contient l'estimation initiale et reçoit la solution.
pub fn cgls(a: &Csr, b: &[f64], x: &mut [f64], max_iter: usize, tol: f64) -> usize {
    let n = a.cols;
    // Préconditionneur : D = 1 / ||colonne||
    let mut colnorm = vec![0.0; n];
    for k in 0..a.val.len() {
        colnorm[a.col[k] as usize] += a.val[k] * a.val[k];
    }
    let d: Vec<f64> = colnorm.iter().map(|&c| if c > 1e-300 { 1.0 / c.sqrt() } else { 0.0 }).collect();

    // On résout en y avec x = D y.
    let mut y: Vec<f64> = x.iter().zip(&d).map(|(&xi, &di)| if di > 0.0 { xi / di } else { 0.0 }).collect();
    let mut tmp = vec![0.0; n];
    let mut ax = vec![0.0; a.rows];
    a.mul(x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
    let mut s = vec![0.0; n];
    a.mul_t(&r, &mut s);
    s.iter_mut().zip(&d).for_each(|(si, di)| *si *= di);
    let mut p = s.clone();
    let mut gamma = dot(&s, &s);
    // Référence du critère d'arrêt : gradient en x = 0 (‖D Aᵀ b‖²). Le gradient initial ne
    // convient pas : avec une bonne estimation de départ il est quasi nul, et le critère
    // relatif ne serait jamais atteint (toutes les itérations seraient faites pour rien).
    let mut atb = vec![0.0; n];
    a.mul_t(b, &mut atb);
    let gamma0 = atb.iter().zip(&d).map(|(v, di)| (v * di) * (v * di)).sum::<f64>().max(gamma).max(1e-300);
    let mut q = vec![0.0; a.rows];
    let mut it = 0;
    while it < max_iter {
        if gamma <= tol * tol * gamma0 {
            break;
        }
        for i in 0..n {
            tmp[i] = d[i] * p[i];
        }
        a.mul(&tmp, &mut q);
        let qq = dot(&q, &q);
        if qq <= 1e-300 {
            break;
        }
        let alpha = gamma / qq;
        for i in 0..n {
            y[i] += alpha * p[i];
        }
        for i in 0..a.rows {
            r[i] -= alpha * q[i];
        }
        a.mul_t(&r, &mut s);
        s.iter_mut().zip(&d).for_each(|(si, di)| *si *= di);
        let gn = dot(&s, &s);
        let beta = gn / gamma;
        gamma = gn;
        for i in 0..n {
            p[i] = s[i] + beta * p[i];
        }
        it += 1;
    }
    for i in 0..n {
        x[i] = d[i] * y[i];
    }
    it
}

/// Matrice symétrique creuse stockée en lignes (voisins + diagonale).
pub struct SymSparse {
    pub diag: Vec<f64>,
    pub nbr: Vec<Vec<(u32, f64)>>,
}

impl SymSparse {
    pub fn mul(&self, x: &[f64], out: &mut [f64]) {
        for i in 0..self.diag.len() {
            let mut s = self.diag[i] * x[i];
            for &(j, w) in &self.nbr[i] {
                s += w * x[j as usize];
            }
            out[i] = s;
        }
    }
}

/// Gradient conjugué préconditionné (Jacobi) pour système SPD.
pub fn pcg(a: &SymSparse, b: &[f64], x: &mut [f64], max_iter: usize, tol: f64) -> usize {
    let n = b.len();
    let mut ax = vec![0.0; n];
    a.mul(x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
    let inv: Vec<f64> = a.diag.iter().map(|&d| if d.abs() > 1e-300 { 1.0 / d } else { 1.0 }).collect();
    let mut z: Vec<f64> = r.iter().zip(&inv).map(|(ri, di)| ri * di).collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let bnorm = dot(b, b).sqrt().max(1e-300);
    let mut ap = vec![0.0; n];
    let mut it = 0;
    while it < max_iter {
        if dot(&r, &r).sqrt() <= tol * bnorm {
            break;
        }
        a.mul(&p, &mut ap);
        let pap = dot(&p, &ap);
        if pap.abs() <= 1e-300 {
            break;
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        for i in 0..n {
            z[i] = r[i] * inv[i];
        }
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        it += 1;
    }
    it
}


// ---------------------------------------------------------------------------
// Équations normales + gradient conjugué préconditionné par Cholesky incomplet IC(0)
// ---------------------------------------------------------------------------

/// Partie triangulaire inférieure (diagonale incluse) d'une matrice symétrique, en CSR trié.
pub struct LowerCsr {
    pub n: usize,
    pub row_start: Vec<usize>,
    pub col: Vec<u32>,
    pub val: Vec<f64>,
}

/// M = AᵀA (partie inférieure), assemblée avec un accumulateur creux.
pub fn normal_lower(a: &Csr) -> LowerCsr {
    let n = a.cols;
    // Aᵀ en CSR : pour chaque colonne de A, les (ligne, valeur)
    let mut cnt = vec![0usize; n + 1];
    for &c in &a.col {
        cnt[c as usize + 1] += 1;
    }
    for i in 0..n {
        cnt[i + 1] += cnt[i];
    }
    let mut at_row = vec![0u32; a.col.len()];
    let mut at_val = vec![0.0; a.col.len()];
    let mut fill = cnt.clone();
    for r in 0..a.rows {
        for k in a.row_start[r]..a.row_start[r + 1] {
            let c = a.col[k] as usize;
            at_row[fill[c]] = r as u32;
            at_val[fill[c]] = a.val[k];
            fill[c] += 1;
        }
    }
    let mut acc = vec![0.0; n];
    let mut mark = vec![usize::MAX; n];
    let mut used: Vec<u32> = Vec::new();
    let mut out = LowerCsr { n, row_start: vec![0], col: Vec::new(), val: Vec::new() };
    for i in 0..n {
        used.clear();
        for k in cnt[i]..cnt[i + 1] {
            let (r, vi) = (at_row[k] as usize, at_val[k]);
            for kk in a.row_start[r]..a.row_start[r + 1] {
                let j = a.col[kk] as usize;
                if j > i {
                    continue;
                }
                if mark[j] != i {
                    mark[j] = i;
                    acc[j] = 0.0;
                    used.push(j as u32);
                }
                acc[j] += vi * a.val[kk];
            }
        }
        used.sort_unstable();
        for &j in &used {
            out.col.push(j);
            out.val.push(acc[j as usize]);
        }
        out.row_start.push(out.col.len());
    }
    out
}

/// Facteur IC(0) : L de même motif que M, M ≈ L Lᵀ. Un décalage diagonal est ajouté en cas
/// de pivot non positif (la factorisation incomplète n'est pas garantie pour toute matrice SDP).
pub fn ichol0(m: &LowerCsr) -> LowerCsr {
    let mut shift = 0.0;
    loop {
        if let Some(l) = try_ichol0(m, shift) {
            return l;
        }
        shift = if shift == 0.0 { 1e-3 } else { shift * 10.0 };
    }
}

fn try_ichol0(m: &LowerCsr, shift: f64) -> Option<LowerCsr> {
    let n = m.n;
    let mut l = LowerCsr { n, row_start: m.row_start.clone(), col: m.col.clone(), val: m.val.clone() };
    let mut diag_pos = vec![0usize; n];
    for i in 0..n {
        let (s, e) = (l.row_start[i], l.row_start[i + 1]);
        diag_pos[i] = e - 1; // colonnes triées : la diagonale est la dernière entrée de la ligne
        if l.col[e - 1] as usize != i {
            return None;
        }
        l.val[e - 1] *= 1.0 + shift;
        let _ = s;
    }
    for i in 0..n {
        let (s, e) = (l.row_start[i], l.row_start[i + 1]);
        for k in s..e {
            let j = l.col[k] as usize;
            // Σ_{p<j} L[i][p]·L[j][p] : produit creux des lignes i et j (fusion triée)
            let (mut a, mut b) = (s, l.row_start[j]);
            let (ae, be) = (k, diag_pos[j]);
            let mut sum = 0.0;
            while a < ae && b < be {
                let (ca, cb) = (l.col[a], l.col[b]);
                if ca == cb {
                    sum += l.val[a] * l.val[b];
                    a += 1;
                    b += 1;
                } else if ca < cb {
                    a += 1;
                } else {
                    b += 1;
                }
            }
            if j == i {
                let d = l.val[k] - sum;
                if d <= 1e-300 || !d.is_finite() {
                    return None;
                }
                l.val[k] = d.sqrt();
            } else {
                l.val[k] = (l.val[k] - sum) / l.val[diag_pos[j]];
            }
        }
    }
    Some(l)
}

fn lower_sym_mul(m: &LowerCsr, x: &[f64], out: &mut [f64]) {
    out.iter_mut().for_each(|v| *v = 0.0);
    for i in 0..m.n {
        for k in m.row_start[i]..m.row_start[i + 1] {
            let j = m.col[k] as usize;
            out[i] += m.val[k] * x[j];
            if j != i {
                out[j] += m.val[k] * x[i];
            }
        }
    }
}

/// z = (L Lᵀ)⁻¹ r
fn ic_apply(l: &LowerCsr, r: &[f64], z: &mut [f64]) {
    let n = l.n;
    // L y = r
    for i in 0..n {
        let (s, e) = (l.row_start[i], l.row_start[i + 1]);
        let mut v = r[i];
        for k in s..e - 1 {
            v -= l.val[k] * z[l.col[k] as usize];
        }
        z[i] = v / l.val[e - 1];
    }
    // Lᵀ x = y (parcours inverse, mise à jour par colonnes)
    for i in (0..n).rev() {
        let (s, e) = (l.row_start[i], l.row_start[i + 1]);
        z[i] /= l.val[e - 1];
        let zi = z[i];
        for k in s..e - 1 {
            z[l.col[k] as usize] -= l.val[k] * zi;
        }
    }
}

/// Moindres carrés min ‖A x − b‖ via les équations normales AᵀA x = Aᵀb, résolues par
/// gradient conjugué préconditionné IC(0). Bien plus rapide que CGLS sur les grands îlots.
pub fn normal_pcg(a: &Csr, b: &[f64], x: &mut [f64], max_iter: usize, tol: f64) -> usize {
    let t0 = std::time::Instant::now();
    let m = normal_lower(a);
    let t1 = t0.elapsed();
    let l = ichol0(&m);
    let t2 = t0.elapsed();
    let n = a.cols;
    let mut rhs = vec![0.0; n];
    a.mul_t(b, &mut rhs);
    let mut mx = vec![0.0; n];
    lower_sym_mul(&m, x, &mut mx);
    let mut r: Vec<f64> = rhs.iter().zip(&mx).map(|(a, b)| a - b).collect();
    let bnorm = dot(&rhs, &rhs).sqrt().max(1e-300);
    let mut z = vec![0.0; n];
    ic_apply(&l, &r, &mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut ap = vec![0.0; n];
    let mut it = 0;
    while it < max_iter {
        if dot(&r, &r).sqrt() <= tol * bnorm {
            break;
        }
        lower_sym_mul(&m, &p, &mut ap);
        let pap = dot(&p, &ap);
        if pap <= 1e-300 {
            break;
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        ic_apply(&l, &r, &mut z);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        it += 1;
    }
    if std::env::var_os("EASYUV_PROFILE").is_some() {
        eprintln!("  [profil]     normal_pcg n={n} nnz={} assemblage {:?} ic0 {:?} total {:?} iter {it}", m.col.len(), t1, t2 - t1, t0.elapsed());
    }
    it
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_pcg_matches_cgls() {
        // petit système surdéterminé déterministe
        let (rows, cols) = (60, 20);
        let mut bld = CsrBuilder::new(cols);
        let mut b = Vec::new();
        for r in 0..rows {
            for k in 0..3 {
                let c = (r * 7 + k * 5) % cols;
                bld.push(c, 1.0 + ((r + k) % 4) as f64 * 0.5);
            }
            bld.end_row();
            b.push(((r * 13) % 9) as f64 - 4.0);
        }
        let a = bld.build();
        let (mut x1, mut x2) = (vec![0.0; cols], vec![0.0; cols]);
        cgls(&a, &b, &mut x1, 10_000, 1e-12);
        normal_pcg(&a, &b, &mut x2, 10_000, 1e-12);
        for (u, v) in x1.iter().zip(&x2) {
            assert!((u - v).abs() < 1e-6, "{u} vs {v}");
        }
    }
}