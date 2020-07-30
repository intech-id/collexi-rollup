// Pedersen hash implementation of the Hasher trait

use std::fmt;

use crate::franklin_crypto::bellman::pairing::ff::PrimeField;
use crate::franklin_crypto::pedersen_hash::{baby_pedersen_hash, Personalization};

use crate::franklin_crypto::alt_babyjubjub::JubjubEngine;
use crate::franklin_crypto::bellman::pairing::bn256::Bn256;

use super::hasher::Hasher;
use crate::primitives::BitIteratorLe;

pub struct PedersenHasher<E: JubjubEngine> {
    params: &'static E::Params,
}

// These implementations are OK as we only have a static reference
// to the constant hasher params, and access is read-only.
unsafe impl<E: JubjubEngine> Send for PedersenHasher<E> {}
unsafe impl<E: JubjubEngine> Sync for PedersenHasher<E> {}

impl<E: JubjubEngine> fmt::Debug for PedersenHasher<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PedersenHasher").finish()
    }
}

// We have to implement `Clone` manually, since deriving it will depend on
// the `Clone` implementation of `E::Params` (and will `.clone()` will not work
// if `E::Params` are not `Clone`), which is redundant: we only hold a reference
// and can just copy it.
impl<E: JubjubEngine> Clone for PedersenHasher<E> {
    fn clone(&self) -> Self {
        Self {
            params: self.params,
        }
    }
}

impl<E: JubjubEngine> Hasher<E::Fr> for PedersenHasher<E> {
    fn hash_bits<I: IntoIterator<Item = bool>>(&self, input: I) -> E::Fr {
        baby_pedersen_hash::<E, _>(Personalization::NoteCommitment, input, &self.params)
            .into_xy()
            .0
        // print!("Leaf hash = {}\n", hash.clone());
    }

    fn hash_elements<I: IntoIterator<Item = E::Fr>>(&self, elements: I) -> E::Fr {
        let mut input = vec![];
        for el in elements.into_iter() {
            input.extend(BitIteratorLe::new(el.into_repr()).take(E::Fr::NUM_BITS as usize));
        }
        baby_pedersen_hash::<E, _>(Personalization::NoteCommitment, input, &self.params)
            .into_xy()
            .0
    }

    fn compress(&self, lhs: &E::Fr, rhs: &E::Fr, i: usize) -> E::Fr {
        let lhs = BitIteratorLe::new(lhs.into_repr()).take(E::Fr::NUM_BITS as usize);
        let rhs = BitIteratorLe::new(rhs.into_repr()).take(E::Fr::NUM_BITS as usize);
        let input = lhs.chain(rhs);
        baby_pedersen_hash::<E, _>(Personalization::MerkleTree(i), input, &self.params)
            .into_xy()
            .0
    }
}

pub type BabyPedersenHasher = PedersenHasher<Bn256>;

impl Default for PedersenHasher<Bn256> {
    fn default() -> Self {
        Self {
            params: &crate::params::JUBJUB_PARAMS,
        }
    }
}

#[test]
fn test_pedersen_hash() {
    let hasher = BabyPedersenHasher::default();

    let hash = hasher.hash_bits(vec![false, false, false, true, true, true, true, true]);
    //debug!("hash:  {:?}", &hash);

    hasher.compress(&hash, &hash, 0);
    //debug!("compr: {:?}", &hash2);

    hasher.compress(&hash, &hash, 1);
    //debug!("compr: {:?}", &hash3);

    //assert_eq!(hasher.empty_hash(),
}
