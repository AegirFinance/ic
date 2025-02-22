//! Proofs of correct chunking

use crate::ni_dkg::fs_ni_dkg::forward_secure::CHUNK_SIZE;
use crate::ni_dkg::fs_ni_dkg::random_oracles::{
    random_oracle, random_oracle_to_scalar, HashedMap, UniqueHash,
};
use arrayvec::ArrayVec;
use ic_crypto_internal_bls12_381_type::{G1Affine, G1Projective, Scalar};
use ic_crypto_internal_types::curves::bls12_381::{Fr as FrBytes, G1 as G1Bytes};
use ic_crypto_internal_types::sign::threshold_sig::ni_dkg::ni_dkg_groth20_bls12_381::ZKProofDec;
use rand::{CryptoRng, Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

/// Domain separators for the zk proof of chunking
const DOMAIN_PROOF_OF_CHUNKING_ORACLE: &str = "ic-zk-proof-of-chunking-chunking";
const DOMAIN_PROOF_OF_CHUNKING_CHALLENGE: &str = "ic-zk-proof-of-chunking-challenge";

const SECURITY_LEVEL: usize = 256;

/// The number of parallel proofs handled by one challenge
///
/// In Section 6.5 of <https://eprint.iacr.org/2021/339.pdf> this
/// value is referred to as `l`
pub const NUM_ZK_REPETITIONS: usize = 32;

/// Defined as ceil(SECURITY_LEVEL/NUM_ZK_REPETITIONS)
pub const CHALLENGE_BITS: usize = (SECURITY_LEVEL + NUM_ZK_REPETITIONS - 1) / NUM_ZK_REPETITIONS;

// The number of bytes needed to represent a challenge (which must fit in a usize)
pub const CHALLENGE_BYTES: usize = (CHALLENGE_BITS + 7) / 8;
const _: () = assert!(CHALLENGE_BYTES < std::mem::size_of::<usize>());

// A bitmask specifyng the size of a challenge
pub const CHALLENGE_MASK: usize = (1 << CHALLENGE_BITS) - 1;

/// Instance for a chunking relation.
///
/// From Section 6.5 of the NIDKG paper.
///   instance = (y=[y_1..y_n], C=[chunk_{1,1}..chunk_{n,m}], R=[R_1,..R_m])
/// We rename:
///   y -> public_keys.
///   C_{i,j} -> ciphertext_chunks.
///   R -> randomizers_r
#[derive(Clone, Debug)]
pub struct ChunkingInstance {
    g1_gen: G1Affine,
    pub public_keys: Vec<G1Affine>,
    //This should be Vec<[G1Affine; NUM_CHUNKS]>
    pub ciphertext_chunks: Vec<Vec<G1Affine>>,
    //This should have size NUM_CHUNKS
    randomizers_r: Vec<G1Affine>,
}

impl ChunkingInstance {
    pub fn new(
        public_keys: Vec<G1Affine>,
        ciphertext_chunks: Vec<Vec<G1Affine>>,
        randomizers_r: Vec<G1Affine>,
    ) -> Self {
        Self {
            g1_gen: *G1Affine::generator(),
            public_keys,
            ciphertext_chunks,
            randomizers_r,
        }
    }
}

/// Witness for the validity of a chunking instance.
///
/// From Section 6.5 of the NIDKG paper:
///   Witness = (scalar_r =[r_1..r_m], scalar_s=[s_{1,1}..s_{n,m}])
#[derive(Clone, Debug)]
pub struct ChunkingWitness {
    //This should have size NUM_CHUNKS
    scalars_r: Vec<Scalar>,
    //This should be Vec<[Scalar; NUM_CHUNKS]>
    scalars_s: Vec<Vec<Scalar>>,
}

impl ChunkingWitness {
    pub fn new(scalars_r: Vec<Scalar>, scalars_s: Vec<Vec<Scalar>>) -> Self {
        Self {
            scalars_r,
            scalars_s,
        }
    }
}

/// Creating or verifying a proof of correct chunking failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZkProofChunkingError {
    InvalidProof,
    InvalidInstance,
}

/// Zero-knowledge proof of chunking.
pub struct ProofChunking {
    y0: G1Affine,
    bb: Vec<G1Affine>,
    cc: Vec<G1Affine>,
    dd: Vec<G1Affine>,
    yy: G1Affine,
    z_r: Vec<Scalar>,
    z_s: Vec<Scalar>,
    z_beta: Scalar,
}

/// First move of the prover in the zero-knowledge proof of chunking.
struct FirstMoveChunking {
    y0: G1Affine,
    bb: Vec<G1Affine>,
    cc: Vec<G1Affine>,
}

/// Prover's response to the first challenge of the verifier.
struct SecondMoveChunking {
    z_s: Vec<Scalar>,
    dd: Vec<G1Affine>,
    yy: G1Affine,
}

impl ChunkingInstance {
    pub fn check_instance(&self) -> Result<(), ZkProofChunkingError> {
        if self.public_keys.is_empty()
            || self.ciphertext_chunks.is_empty()
            || self.randomizers_r.is_empty()
        {
            return Err(ZkProofChunkingError::InvalidInstance);
        };
        if self.public_keys.len() != self.ciphertext_chunks.len() {
            return Err(ZkProofChunkingError::InvalidInstance);
        };
        Ok(())
    }
}

impl FirstMoveChunking {
    fn from(y0: &G1Affine, bb: &[G1Affine], cc: &[G1Affine]) -> Self {
        Self {
            y0: y0.to_owned(),
            bb: bb.to_owned(),
            cc: cc.to_owned(),
        }
    }
}

impl SecondMoveChunking {
    fn from(z_s: &[Scalar], dd: &[G1Affine], yy: &G1Affine) -> Self {
        Self {
            z_s: z_s.to_owned(),
            dd: dd.to_owned(),
            yy: yy.to_owned(),
        }
    }
}

impl UniqueHash for ChunkingInstance {
    fn unique_hash(&self) -> [u8; 32] {
        let mut map = HashedMap::new();
        map.insert_hashed("g1-generator", &self.g1_gen);
        map.insert_hashed("public-keys", &self.public_keys);
        map.insert_hashed("ciphertext-chunks", &self.ciphertext_chunks);
        map.insert_hashed("randomizers-r", &self.randomizers_r);
        map.unique_hash()
    }
}

impl UniqueHash for FirstMoveChunking {
    fn unique_hash(&self) -> [u8; 32] {
        let mut map = HashedMap::new();
        map.insert_hashed("y0", &self.y0);
        map.insert_hashed("bb", &self.bb);
        map.insert_hashed("cc", &self.cc);
        map.unique_hash()
    }
}

impl UniqueHash for SecondMoveChunking {
    fn unique_hash(&self) -> [u8; 32] {
        let mut map = HashedMap::new();
        map.insert_hashed("z_s", &self.z_s);
        map.insert_hashed("dd", &self.dd);
        map.insert_hashed("yy", &self.yy);
        map.unique_hash()
    }
}

/// Create a proof of correct chunking
pub fn prove_chunking<R: RngCore + CryptoRng>(
    instance: &ChunkingInstance,
    witness: &ChunkingWitness,
    rng: &mut R,
) -> ProofChunking {
    instance
        .check_instance()
        .expect("The chunking proof instance is invalid");

    let spec_m = instance.randomizers_r.len();
    let spec_n = instance.public_keys.len();

    let ss = spec_n * spec_m * ((CHUNK_SIZE as usize) - 1) * CHALLENGE_MASK;
    let zz = 2 * NUM_ZK_REPETITIONS * ss;
    let range = zz - 1 + ss + 1;
    let zz_big = Scalar::from_usize(zz);
    let p_sub_s = Scalar::from_usize(ss).neg();

    // y0 <- getRandomG1
    let y0 = G1Affine::hash(b"ic-crypto-nizk-chunking-proof-y0", &rng.gen::<[u8; 32]>());

    let g1 = instance.g1_gen;

    // sigma = replicateM NUM_ZK_REPETITIONS $ getRandom [-S..Z-1]
    // beta = replicateM NUM_ZK_REPETITIONS $ getRandom [0..p-1]
    // bb = map (g1^) beta
    // cc = zipWith (\x pk -> y0^x * g1^pk) beta sigma
    let beta: Vec<Scalar> = (0..NUM_ZK_REPETITIONS)
        .map(|_| Scalar::random(rng))
        .collect();
    let bb: Vec<G1Affine> = beta
        .iter()
        .map(|beta_i| G1Affine::from(g1 * beta_i))
        .collect();

    let (first_move, first_challenge, z_s) = loop {
        let sigma: Vec<Scalar> = (0..NUM_ZK_REPETITIONS)
            .map(|_| Scalar::random_within_range(rng, range as u64) + p_sub_s)
            .collect();
        let cc: Vec<G1Affine> = beta
            .iter()
            .zip(&sigma)
            .map(|(beta_i, sigma_i)| {
                G1Projective::mul2(&y0.into(), beta_i, &g1.into(), sigma_i).to_affine()
            })
            .collect();

        let first_move = FirstMoveChunking::from(&y0, &bb, &cc);
        // Verifier's challenge.
        let first_challenge =
            ChunksOracle::new(instance, &first_move).get_all_chunks(spec_n, spec_m);

        // z_s = [sum [e_ijk * s_ij | i <- [1..n], j <- [1..m]] + sigma_k | k <- [1..l]]
        let z_s: Result<Vec<Scalar>, ()> = (0..NUM_ZK_REPETITIONS)
            .map(|k| {
                let mut acc = Scalar::zero();
                first_challenge
                    .iter()
                    .zip(witness.scalars_s.iter())
                    .for_each(|(e_i, s_i)| {
                        e_i.iter().zip(s_i.iter()).for_each(|(e_ij, s_ij)| {
                            acc += Scalar::from_usize(e_ij[k]) * s_ij;
                        });
                    });
                acc += sigma[k];

                if acc > zz_big {
                    Err(())
                } else {
                    Ok(acc)
                }
            })
            .collect();

        if let Ok(z_s) = z_s {
            break (first_move, first_challenge, z_s);
        }
    };

    // delta <- replicate (n + 1) getRandom
    // dd = map (g1^) delta
    // Y = product [y_i^delta_i | i <- [0..n]]
    let mut delta = Vec::with_capacity(spec_n + 1);
    let mut dd = Vec::with_capacity(spec_n + 1);
    let mut yy = *G1Projective::identity();
    for i in 0..spec_n + 1 {
        let delta_i = Scalar::random(rng);
        dd.push(G1Affine::from(g1 * delta_i));
        if i == 0 {
            yy = y0 * delta_i;
        } else {
            yy += instance.public_keys[i - 1] * delta_i;
        }
        delta.push(delta_i);
    }

    let yy = G1Affine::from(yy);

    let second_move = SecondMoveChunking::from(&z_s, &dd, &yy);

    // Second verifier's challege. Forth move in the protocol.
    // x = oracle(e, z_s, dd, yy)
    let second_challenge = chunking_proof_challenge_oracle(&first_challenge, &second_move);

    let mut z_r = Vec::new();
    let mut delta_idx = 1;
    for e_i in first_challenge.iter() {
        let mut z_rk = delta[delta_idx];
        delta_idx += 1;
        e_i.iter()
            .zip(witness.scalars_r.iter())
            .for_each(|(e_ij, r_j)| {
                let mut xpow = second_challenge;
                e_ij.iter().for_each(|e_ijk| {
                    z_rk += Scalar::from_usize(*e_ijk) * r_j * xpow;
                    xpow *= second_challenge;
                })
            });
        z_r.push(z_rk);
    }

    let mut xpow = second_challenge;
    let mut z_beta = delta[0];
    beta.iter().for_each(|beta_k| {
        z_beta += *beta_k * xpow;
        xpow *= second_challenge;
    });

    ProofChunking {
        y0: first_move.y0,
        bb: first_move.bb,
        cc: first_move.cc,
        dd,
        yy,
        z_r,
        z_s,
        z_beta,
    }
}

/// Verify a proof of correct chunking
pub fn verify_chunking(
    instance: &ChunkingInstance,
    nizk: &ProofChunking,
) -> Result<(), ZkProofChunkingError> {
    instance.check_instance()?;

    let num_receivers = instance.public_keys.len();
    require_eq("bb", nizk.bb.len(), NUM_ZK_REPETITIONS)?;
    require_eq("cc", nizk.cc.len(), NUM_ZK_REPETITIONS)?;
    require_eq("dd", nizk.dd.len(), num_receivers + 1)?;
    require_eq("z_r", nizk.z_r.len(), num_receivers)?;
    require_eq("z_s", nizk.z_s.len(), NUM_ZK_REPETITIONS)?;

    let spec_m = instance.randomizers_r.len();
    let spec_n = instance.public_keys.len();
    let ss = spec_n * spec_m * (CHUNK_SIZE as usize - 1) * CHALLENGE_MASK;
    let zz = 2 * NUM_ZK_REPETITIONS * ss;
    let zz_big = Scalar::from_usize(zz);

    for z_sk in nizk.z_s.iter() {
        if z_sk >= &zz_big {
            return Err(ZkProofChunkingError::InvalidProof);
        }
    }

    let first_move = FirstMoveChunking::from(&nizk.y0, &nizk.bb, &nizk.cc);
    let second_move = SecondMoveChunking::from(&nizk.z_s, &nizk.dd, &nizk.yy);
    // e_{m,n,l} = oracle(instance, y_0, bb, cc)
    let e = ChunksOracle::new(instance, &first_move).get_all_chunks(spec_n, spec_m);

    // x = oracle(e, z_s, dd, yy)
    let x = chunking_proof_challenge_oracle(&e, &second_move);

    let xpowers = Scalar::xpowers(&x, NUM_ZK_REPETITIONS);
    let g1 = instance.g1_gen;

    // Verify: all [product [R_j ^ sum [e_ijk * x^k | k <- [1..l]] | j <- [1..m]] *
    // dd_i == g1 ^ z_r_i | i <- [1..n]]
    let mut delta_idx = 0;
    let mut verifies = true;

    e.iter().zip(nizk.z_r.iter()).for_each(|(e_i, z_ri)| {
        delta_idx += 1;

        let e_ijk_polynomials: Vec<_> = e_i
            .iter()
            .map(|e_ij| {
                let mut acc = Scalar::zero();
                e_ij.iter().enumerate().for_each(|(k, e_ijk)| {
                    acc += Scalar::from_usize(*e_ijk) * xpowers[k];
                });

                acc
            })
            .collect();

        let lhs = G1Projective::muln_affine_vartime(&instance.randomizers_r, &e_ijk_polynomials)
            + nizk.dd[delta_idx];

        let rhs = g1 * z_ri;
        verifies = verifies && (lhs == rhs);
    });
    if !verifies {
        return Err(ZkProofChunkingError::InvalidProof);
    }

    // Verify: product [bb_k ^ x^k | k <- [1..l]] * dd_0 == g1 ^ z_beta
    let lhs = G1Projective::muln_affine_vartime(&nizk.bb, &xpowers) + nizk.dd[0];

    let rhs = g1 * nizk.z_beta;
    if lhs != rhs {
        return Err(ZkProofChunkingError::InvalidProof);
    }

    // Verify: product [product [chunk_ij ^ e_ijk | i <- [1..n], j <- [1..m]] ^ x^k
    // | k <- [1..l]] * product [cc_k ^ x^k | k <- [1..l]] * Y   = product
    // [y_i^z_ri | i <- [1..n]] * y0^z_beta * g_1 ^ sum [z_sk * x^k | k <- [1..l]]

    let cij_to_eijks: Vec<G1Projective> = (0..NUM_ZK_REPETITIONS)
        .map(|k| {
            let c_ij_s: Vec<_> = instance
                .ciphertext_chunks
                .iter()
                .flatten()
                .cloned()
                .collect();
            let e_ijk_s: Vec<_> = e
                .iter()
                .flatten()
                .map(|e_ij| Scalar::from_usize(e_ij[k]))
                .collect();
            if c_ij_s.len() != spec_m * spec_n || e_ijk_s.len() != spec_m * spec_n {
                return Err(ZkProofChunkingError::InvalidProof);
            }

            Ok(G1Projective::muln_affine_vartime(&c_ij_s, &e_ijk_s) + nizk.cc[k])
        })
        .collect::<Result<Vec<_>, _>>()?;

    let cij_to_eijks_terms = cij_to_eijks
        .iter()
        .cloned()
        .zip(xpowers.clone())
        .collect::<Vec<_>>();

    let lhs = G1Projective::muln_vartime(&cij_to_eijks_terms) + nizk.yy;

    let acc = Scalar::muln_vartime(&nizk.z_s.iter().cloned().zip(xpowers).collect::<Vec<_>>());

    let rhs = G1Projective::muln_affine_vartime(&instance.public_keys, &nizk.z_r)
        + G1Projective::mul2(&nizk.y0.into(), &nizk.z_beta, &g1.into(), &acc);

    if lhs != rhs {
        return Err(ZkProofChunkingError::InvalidProof);
    }
    Ok(())
}

struct ChunksOracle {
    rng: ChaCha20Rng, // The choice of RNG matters so this is explicit, not a trait.
}

impl ChunksOracle {
    pub fn new(instance: &ChunkingInstance, first_move: &FirstMoveChunking) -> Self {
        let mut map = HashedMap::new();
        map.insert_hashed("instance", instance);
        map.insert_hashed("first-move", first_move);
        map.insert_hashed("number-of-parallel-repetitions", &NUM_ZK_REPETITIONS);

        let hash = random_oracle(DOMAIN_PROOF_OF_CHUNKING_ORACLE, &map);

        let rng = ChaCha20Rng::from_seed(hash);
        Self { rng }
    }

    fn getbyte(&mut self) -> u8 {
        let mut random_byte: [u8; 1] = [0; 1];
        // `fill_bytes()` with 1-byte buffer consumes 4 bytes of the random stream.
        self.rng.fill_bytes(&mut random_byte);
        random_byte[0]
    }

    /// Get a chunk-sized unit of data.
    fn get_chunk(&mut self) -> usize {
        // The order of the getbyte(..) calls matters so this is intentionally serial.
        CHALLENGE_MASK
            & (0..CHALLENGE_BYTES).fold(0, |state, _| (state << 8) | (self.getbyte() as usize))
    }

    fn get_all_chunks(&mut self, spec_n: usize, spec_m: usize) -> Vec<Vec<Vec<usize>>> {
        (0..spec_n)
            .map(|_| {
                (0..spec_m)
                    .map(|_| (0..NUM_ZK_REPETITIONS).map(|_| self.get_chunk()).collect())
                    .collect()
            })
            .collect()
    }
}

fn chunking_proof_challenge_oracle(
    first_challenge: &[Vec<Vec<usize>>],
    second_move: &SecondMoveChunking,
) -> Scalar {
    let mut map = HashedMap::new();
    map.insert_hashed("first-challenge", &first_challenge.to_vec());
    map.insert_hashed("second-move", second_move);

    random_oracle_to_scalar(DOMAIN_PROOF_OF_CHUNKING_CHALLENGE, &map)
}

#[inline]
fn require_eq(
    name: &'static str,
    actual: usize,
    expected: usize,
) -> Result<(), ZkProofChunkingError> {
    if expected != actual {
        dbg!(name);
        dbg!(actual);
        dbg!(expected);
        Err(ZkProofChunkingError::InvalidProof)
    } else {
        Ok(())
    }
}

impl ProofChunking {
    /// Serialises a chunking proof from the miracl-compatible form to the stanard
    /// form.
    ///
    /// # Panics
    /// This will panic if the miracl proof is malformed.  Given that the miracl
    /// representation is created internally, such an error can only be caused by an
    /// error in implementation.
    pub fn serialize(&self) -> ZKProofDec {
        ZKProofDec {
            first_move_y0: self.y0.serialize_to::<G1Bytes>(),
            first_move_b: self
                .bb
                .iter()
                .map(|g1| g1.serialize_to::<G1Bytes>())
                .collect::<ArrayVec<_>>()
                .into_inner()
                .expect("Wrong size of first_move_b==bb in chunking proof"),
            first_move_c: self
                .cc
                .iter()
                .map(|g1| g1.serialize_to::<G1Bytes>())
                .collect::<ArrayVec<_>>()
                .into_inner()
                .expect("Wrong size of first_move_c==cc in chunking proof"),
            second_move_d: self
                .dd
                .iter()
                .map(|g1| g1.serialize_to::<G1Bytes>())
                .collect(),
            second_move_y: self.yy.serialize_to::<G1Bytes>(),
            response_z_r: self
                .z_r
                .iter()
                .map(|s| s.serialize_to::<FrBytes>())
                .collect(),
            response_z_s: self
                .z_s
                .iter()
                .map(|s| s.serialize_to::<FrBytes>())
                .collect::<ArrayVec<_>>()
                .into_inner()
                .expect("Wrong size of response_z_s==z_s in chunking proof"),
            response_z_b: self.z_beta.serialize_to::<FrBytes>(),
        }
    }

    /// Parses a chunking proof from the standard form
    pub fn deserialize(proof: &ZKProofDec) -> Option<Self> {
        let y0 = G1Affine::deserialize(&proof.first_move_y0);
        let bb = G1Affine::batch_deserialize(&proof.first_move_b[..]);
        let cc = G1Affine::batch_deserialize(&proof.first_move_c[..]);
        let dd = G1Affine::batch_deserialize(&proof.second_move_d[..]);
        let yy = G1Affine::deserialize(proof.second_move_y.as_bytes());
        let z_r = Scalar::batch_deserialize(&proof.response_z_r);
        let z_s = Scalar::batch_deserialize(&proof.response_z_s[..]);
        let z_beta = Scalar::deserialize(proof.response_z_b.as_bytes());

        if let (Ok(y0), Ok(bb), Ok(cc), Ok(dd), Ok(yy), Ok(z_r), Ok(z_s), Ok(z_beta)) =
            (y0, bb, cc, dd, yy, z_r, z_s, z_beta)
        {
            if dd.len() != z_r.len() + 1 {
                return None;
            }

            Some(Self {
                y0,
                bb,
                cc,
                dd,
                yy,
                z_r,
                z_s,
                z_beta,
            })
        } else {
            None
        }
    }
}
