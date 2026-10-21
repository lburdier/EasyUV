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
    let gamma0 = gamma.max(1e-300);
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
