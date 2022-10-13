use anyhow::Result;
use eth_trie_utils::partial_trie::{Nibbles, PartialTrie};
use ethereum_types::{BigEndianHash, H256};

use super::nibbles;
use crate::cpu::kernel::aggregator::KERNEL;
use crate::cpu::kernel::constants::global_metadata::GlobalMetadata;
use crate::cpu::kernel::interpreter::Interpreter;
use crate::cpu::kernel::tests::mpt::{test_account_1_rlp, test_account_2_rlp};
use crate::generation::mpt::{all_mpt_prover_inputs_reversed, AccountRlp};
use crate::generation::TrieInputs;

#[test]
fn mpt_insert_empty() -> Result<()> {
    test_state_trie(Default::default(), nibbles(0xABC), test_account_2_rlp())
}

#[test]
fn mpt_insert_leaf_identical_keys() -> Result<()> {
    let key = nibbles(0xABC);
    let state_trie = PartialTrie::Leaf {
        nibbles: key,
        value: test_account_1_rlp(),
    };
    test_state_trie(state_trie, key, test_account_2_rlp())
}

#[test]
fn mpt_insert_leaf_nonoverlapping_keys() -> Result<()> {
    let state_trie = PartialTrie::Leaf {
        nibbles: nibbles(0xABC),
        value: test_account_1_rlp(),
    };
    test_state_trie(state_trie, nibbles(0x123), test_account_2_rlp())
}

#[test]
fn mpt_insert_leaf_overlapping_keys() -> Result<()> {
    let state_trie = PartialTrie::Leaf {
        nibbles: nibbles(0xABC),
        value: test_account_1_rlp(),
    };
    test_state_trie(state_trie, nibbles(0xADE), test_account_2_rlp())
}

#[test]
fn mpt_insert_leaf_insert_key_extends_leaf_key() -> Result<()> {
    let state_trie = PartialTrie::Leaf {
        nibbles: nibbles(0xABC),
        value: test_account_1_rlp(),
    };
    test_state_trie(state_trie, nibbles(0xABCDE), test_account_2_rlp())
}

#[test]
fn mpt_insert_leaf_leaf_key_extends_insert_key() -> Result<()> {
    let state_trie = PartialTrie::Leaf {
        nibbles: nibbles(0xABCDE),
        value: test_account_1_rlp(),
    };
    test_state_trie(state_trie, nibbles(0xABC), test_account_2_rlp())
}

#[test]
fn mpt_insert_branch_replacing_empty_child() -> Result<()> {
    let children = std::array::from_fn(|_| PartialTrie::Empty.into());
    let state_trie = PartialTrie::Branch {
        children,
        value: vec![],
    };

    test_state_trie(state_trie, nibbles(0xABC), test_account_2_rlp())
}

#[test]
fn mpt_insert_extension_nonoverlapping_keys() -> Result<()> {
    // Existing keys are 0xABC, 0xABCDEF; inserted key is 0x12345.
    let mut children = std::array::from_fn(|_| PartialTrie::Empty.into());
    children[0xD] = PartialTrie::Leaf {
        nibbles: nibbles(0xEF),
        value: test_account_1_rlp(),
    }
    .into();
    let state_trie = PartialTrie::Extension {
        nibbles: nibbles(0xABC),
        child: PartialTrie::Branch {
            children,
            value: test_account_1_rlp(),
        }
        .into(),
    };
    test_state_trie(state_trie, nibbles(0x12345), test_account_2_rlp())
}

#[test]
fn mpt_insert_extension_insert_key_extends_node_key() -> Result<()> {
    // Existing keys are 0xA, 0xABCD; inserted key is 0xABCDEF.
    let mut children = std::array::from_fn(|_| PartialTrie::Empty.into());
    children[0xB] = PartialTrie::Leaf {
        nibbles: nibbles(0xCD),
        value: test_account_1_rlp(),
    }
    .into();
    let state_trie = PartialTrie::Extension {
        nibbles: nibbles(0xA),
        child: PartialTrie::Branch {
            children,
            value: test_account_1_rlp(),
        }
        .into(),
    };
    test_state_trie(state_trie, nibbles(0xABCDEF), test_account_2_rlp())
}

#[test]
fn mpt_insert_branch_to_leaf_same_key() -> Result<()> {
    let leaf = PartialTrie::Leaf {
        nibbles: nibbles(0xBCD),
        value: test_account_1_rlp(),
    }
    .into();
    let mut children = std::array::from_fn(|_| PartialTrie::Empty.into());
    children[0xA] = leaf;
    let state_trie = PartialTrie::Branch {
        children,
        value: vec![],
    };

    test_state_trie(state_trie, nibbles(0xABCD), test_account_2_rlp())
}

fn test_state_trie(state_trie: PartialTrie, k: Nibbles, v: Vec<u8>) -> Result<()> {
    let trie_inputs = TrieInputs {
        state_trie: state_trie.clone(),
        transactions_trie: Default::default(),
        receipts_trie: Default::default(),
        storage_tries: vec![],
    };
    let load_all_mpts = KERNEL.global_labels["load_all_mpts"];
    let mpt_insert_state_trie = KERNEL.global_labels["mpt_insert_state_trie"];
    let mpt_hash_state_trie = KERNEL.global_labels["mpt_hash_state_trie"];

    let initial_stack = vec![0xDEADBEEFu32.into()];
    let mut interpreter = Interpreter::new_with_kernel(load_all_mpts, initial_stack);
    interpreter.generation_state.mpt_prover_inputs = all_mpt_prover_inputs_reversed(&trie_inputs);
    interpreter.run()?;
    assert_eq!(interpreter.stack(), vec![]);

    // Next, execute mpt_insert_state_trie.
    interpreter.offset = mpt_insert_state_trie;
    let trie_data = interpreter.get_trie_data_mut();
    if trie_data.is_empty() {
        // In the assembly we skip over 0, knowing trie_data[0] = 0 by default.
        // Since we don't explicitly set it to 0, we need to do so here.
        trie_data.push(0.into());
    }
    let value_ptr = trie_data.len();
    let account: AccountRlp = rlp::decode(&v).expect("Decoding failed");
    let account_data = account.to_vec();
    trie_data.push(account_data.len().into());
    trie_data.extend(account_data);
    let trie_data_len = trie_data.len().into();
    interpreter.set_global_metadata_field(GlobalMetadata::TrieDataSize, trie_data_len);
    interpreter.push(0xDEADBEEFu32.into());
    interpreter.push(value_ptr.into()); // value_ptr
    interpreter.push(k.packed); // key
    interpreter.push(k.count.into()); // num_nibbles

    interpreter.run()?;
    assert_eq!(
        interpreter.stack().len(),
        0,
        "Expected empty stack after insert, found {:?}",
        interpreter.stack()
    );

    // Now, execute mpt_hash_state_trie.
    interpreter.offset = mpt_hash_state_trie;
    interpreter.push(0xDEADBEEFu32.into());
    interpreter.run()?;

    assert_eq!(
        interpreter.stack().len(),
        1,
        "Expected 1 item on stack after hashing, found {:?}",
        interpreter.stack()
    );
    let hash = H256::from_uint(&interpreter.stack()[0]);

    let updated_trie = state_trie.insert(k, v);
    let expected_state_trie_hash = updated_trie.calc_hash();
    assert_eq!(hash, expected_state_trie_hash);

    Ok(())
}