#![allow(clippy::option_env_unwrap)]
// Built-in deps
use std::env;
use std::str::FromStr;
// External deps
use crate::franklin_crypto::alt_babyjubjub::AltJubjubBn256;
use lazy_static::lazy_static;
// Workspace deps
use crate::config_options::parse_env;
use crate::franklin_crypto::group_hash::BlakeHasher;
use crate::franklin_crypto::rescue::bn256::Bn256RescueParams;
use crate::merkle_tree::pedersen_hasher::BabyPedersenHasher;
use crate::merkle_tree::rescue_hasher::BabyRescueHasher;
use crate::node::TokenId;

static mut ACCOUNT_TREE_DEPTH_VALUE: usize = 24;
/// account_tree_depth.
/// Value must be specified as environment variable at compile time under `ACCOUNT_TREE_DEPTH_VALUE` key.
pub fn account_tree_depth() -> usize {
    // use of mutable static is unsafe as it can be mutated by multiple threads.
    // There's no risk of data race, the worst that can happen is that we parse
    // and set environment value multuple times, which is ok.

    unsafe {
        if ACCOUNT_TREE_DEPTH_VALUE == 0 {
            #[allow(clippy::option_env_unwrap)]
            let value: &'static str = option_env!("ACCOUNT_TREE_DEPTH")
                .expect("ACCOUNT_TREE_DEPTH variable was not set during compilation. \
                        Make sure that ACCOUNT_TREE_DEPTH is set in `dev.env` file and recompile the project");
            ACCOUNT_TREE_DEPTH_VALUE =
                usize::from_str(value).expect("ACCOUNT_TREE_DEPTH compile value is invalid");
            let runtime_value = parse_env::<usize>("ACCOUNT_TREE_DEPTH");
            if runtime_value != ACCOUNT_TREE_DEPTH_VALUE {
                panic!(
                    "ACCOUNT_TREE_DEPTH want runtime value: {}, got: {}",
                    ACCOUNT_TREE_DEPTH_VALUE, runtime_value
                );
            }
        }
        ACCOUNT_TREE_DEPTH_VALUE
    }
}

static mut TOKEN_TREE_DEPTH_VALUE: usize = 8;
/// balance tree_depth.
/// Value must be specified as environment variable at compile time under `BALANCE_TREE_DEPTH_VALUE` key.
pub fn token_tree_depth() -> usize {
    // use of mutable static is unsafe as it can be mutated by multiple threads.
    // There's no risk of data race, the worst that can happen is that we parse
    // and set environment value multuple times, which is ok.

    unsafe {
        if TOKEN_TREE_DEPTH_VALUE == 0 {
            #[allow(clippy::option_env_unwrap)]
            let value: &'static str = option_env!("TOKEN_TREE_DEPTH")
                .expect("TOKEN_TREE_DEPTH variable was not set during compilation. \
                        Make sure that TOKEN_TREE_DEPTH is set in `dev.env` file and recompile the project");
            TOKEN_TREE_DEPTH_VALUE =
                usize::from_str(value).expect("TOKEN_TREE_DEPTH compile value is invalid");
            let runtime_value = parse_env::<usize>("TOKEN_TREE_DEPTH");
            if runtime_value != TOKEN_TREE_DEPTH_VALUE {
                panic!(
                    "TOKEN_TREE_DEPTH want runtime value: {}, got: {}",
                    TOKEN_TREE_DEPTH_VALUE, runtime_value
                );
            }
        }
        TOKEN_TREE_DEPTH_VALUE
    }
}
/// Number of supported tokens.
pub fn total_tokens() -> usize {
    2usize.pow(token_tree_depth() as u32)
}
pub const ETH_TOKEN_ID: TokenId = 0;

pub const ACCOUNT_ID_BIT_WIDTH: usize = 24;

pub const INPUT_DATA_ADDRESS_BYTES_WIDTH: usize = 32;
pub const INPUT_DATA_BLOCK_NUMBER_BYTES_WIDTH: usize = 32;
pub const INPUT_DATA_FEE_ACC_BYTES_WIDTH_WITH_EMPTY_OFFSET: usize = 32;
pub const INPUT_DATA_FEE_ACC_BYTES_WIDTH: usize = 3;
pub const INPUT_DATA_ROOT_BYTES_WIDTH: usize = 32;
pub const INPUT_DATA_EMPTY_BYTES_WIDTH: usize = 64;
pub const INPUT_DATA_ROOT_HASH_BYTES_WIDTH: usize = 32;

pub const TX_TYPE_BIT_WIDTH: usize = 8;

/// Account subtree hash width
pub const SUBTREE_HASH_WIDTH: usize = 254; //seems to be equal to Bn256::NUM_BITS could be replaced
pub const SUBTREE_HASH_WIDTH_PADDED: usize = 256;

/// token_id bit width
pub const TOKENID_BIT_WIDTH: usize = 16;
pub const BALANCE_BIT_WIDTH: usize = 128;

pub const NEW_PUBKEY_HASH_WIDTH: usize = FR_ADDRESS_LEN * 8;
pub const ADDRESS_WIDTH: usize = FR_ADDRESS_LEN * 8;
/// Nonce bit width
pub const NONCE_BIT_WIDTH: usize = 32;
//
pub const CHUNK_BIT_WIDTH: usize = 64;

pub const MAX_CIRCUIT_MSG_HASH_BITS: usize = 736;

pub const ETH_ADDRESS_BIT_WIDTH: usize = 160;
/// Block number bit width
pub const BLOCK_NUMBER_BIT_WIDTH: usize = 32;

/// Amount bit widths
pub const AMOUNT_EXPONENT_BIT_WIDTH: usize = 5;
pub const AMOUNT_MANTISSA_BIT_WIDTH: usize = 35;

/// Fee bit widths
pub const FEE_EXPONENT_BIT_WIDTH: usize = 5;
pub const FEE_MANTISSA_BIT_WIDTH: usize = 11;

// Signature data
pub const SIGNATURE_S_BIT_WIDTH: usize = 254;
pub const SIGNATURE_S_BIT_WIDTH_PADDED: usize = 256;
pub const SIGNATURE_R_X_BIT_WIDTH: usize = 254;
pub const SIGNATURE_R_Y_BIT_WIDTH: usize = 254;
pub const SIGNATURE_R_BIT_WIDTH_PADDED: usize = 256;

// Fr element encoding
pub const FR_BIT_WIDTH: usize = 254;
pub const FR_BIT_WIDTH_PADDED: usize = 256;

pub const LEAF_DATA_BIT_WIDTH: usize =
    NONCE_BIT_WIDTH + NEW_PUBKEY_HASH_WIDTH + FR_BIT_WIDTH_PADDED + ETH_ADDRESS_BIT_WIDTH;

static mut BLOCK_CHUNK_SIZES_VALUE: Vec<usize> = Vec::new();

pub(crate) fn block_chunk_sizes() -> &'static [usize] {
    // use of mutable static is unsafe as it can be mutated by multiple threads.
    // using `unsafe` block as there's no risk of data race, the worst that can
    // happen is we read and set environment value multuple times, which is ok.
    unsafe {
        if BLOCK_CHUNK_SIZES_VALUE.is_empty() {
            let runtime_value = env::var("BLOCK_CHUNK_SIZES").expect("BLOCK_CHUNK_SIZES missing");
            BLOCK_CHUNK_SIZES_VALUE = runtime_value
                .split(',')
                .map(|s| usize::from_str(s).unwrap())
                .collect::<Vec<_>>();
        }
        BLOCK_CHUNK_SIZES_VALUE.as_slice()
    }
}

/// Priority op should be executed for this number of eth blocks.
pub const PRIORITY_EXPIRATION: u64 = 250;
pub const FR_ADDRESS_LEN: usize = 20;

pub const PAD_MSG_BEFORE_HASH_BITS_LEN: usize = 736;

/// Size of the data that is signed for withdraw tx
pub const SIGNED_WITHDRAW_BIT_WIDTH: usize = TX_TYPE_BIT_WIDTH
    + ACCOUNT_ID_BIT_WIDTH
    + 2 * ADDRESS_WIDTH
    + TOKENID_BIT_WIDTH
    + FEE_EXPONENT_BIT_WIDTH
    + FEE_MANTISSA_BIT_WIDTH
    + NONCE_BIT_WIDTH;

/// Size of the data that is signed for transfer tx
pub const SIGNED_TRANSFER_BIT_WIDTH: usize = TX_TYPE_BIT_WIDTH
    + ACCOUNT_ID_BIT_WIDTH
    + 2 * ADDRESS_WIDTH
    + TOKENID_BIT_WIDTH
    + FEE_EXPONENT_BIT_WIDTH
    + FEE_MANTISSA_BIT_WIDTH
    + NONCE_BIT_WIDTH;

lazy_static! {
    pub static ref JUBJUB_PARAMS: AltJubjubBn256 = AltJubjubBn256::new();
    pub static ref PEDERSEN_HASHER: BabyPedersenHasher = BabyPedersenHasher::default();
    pub static ref RESCUE_PARAMS: Bn256RescueParams =
        Bn256RescueParams::new_2_into_1::<BlakeHasher>();
    pub static ref RESCUE_HASHER: BabyRescueHasher = BabyRescueHasher::default();
}
