use crate::Fr;
use bus_mapping::{
    circuit_input_builder::{BuilderClient, CircuitsParams},
    mock::BlockData,
    rpc::GethClient,
};
use eth_types::{geth_types, geth_types::GethData, Address, ToBigEndian, Word, H256};
use ethers_providers::Http;
use std::str::FromStr;
use zkevm_circuits::{evm_circuit, pi_circuit::PublicData};
use zkevm_common::prover::CircuitConfig;

/// Wrapper struct for circuit witness data.
pub struct CircuitWitness {
    pub circuit_config: CircuitConfig,
    pub eth_block: eth_types::Block<eth_types::Transaction>,
    pub block: bus_mapping::circuit_input_builder::Block,
    pub code_db: bus_mapping::state_db::CodeDB,
}

impl CircuitWitness {
    pub fn dummy(circuit_config: CircuitConfig) -> Result<Self, String> {
        let history_hashes = vec![Word::zero(); 256];
        let mut eth_block: eth_types::Block<eth_types::Transaction> = eth_types::Block::default();
        eth_block.author = Some(Address::zero());
        eth_block.number = Some(history_hashes.len().into());
        eth_block.base_fee_per_gas = Some(0.into());
        eth_block.hash = Some(eth_block.parent_hash);
        eth_block.gas_limit = circuit_config.block_gas_limit.into();

        let circuit_params = CircuitsParams {
            max_txs: circuit_config.max_txs,
            max_calldata: circuit_config.max_calldata,
            max_bytecode: circuit_config.max_bytecode,
            max_rws: circuit_config.max_rws,
            max_copy_rows: circuit_config.max_copy_rows,
            max_inner_blocks: circuit_config.max_inner_blocks,
            max_exp_steps: circuit_config.max_exp_steps,
            max_evm_rows: circuit_config.pad_to,
            max_keccak_rows: circuit_config.keccak_padding,
        };
        let empty_data = GethData {
            chain_id: Word::from(99),
            history_hashes: vec![Word::zero(); 256],
            eth_block,
            geth_traces: Vec::new(),
            accounts: Vec::new(),
        };
        let mut builder =
            BlockData::new_from_geth_data_with_params(empty_data.clone(), circuit_params)
                .new_circuit_input_builder();
        builder
            .handle_block(&empty_data.eth_block, &empty_data.geth_traces)
            .unwrap();
        Ok(Self {
            circuit_config,
            eth_block: empty_data.eth_block,
            block: builder.block,
            code_db: builder.code_db,
        })
    }

    /// Gathers debug trace(s) from `rpc_url` for block `block_num`.
    /// Expects a go-ethereum node with debug & archive capabilities on `rpc_url`.
    pub async fn from_rpc(
        block_num: &u64,
        rpc_url: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let url = Http::from_str(rpc_url)?;
        let geth_client = GethClient::new(url);
        println!("=============>url: {:?}", rpc_url);
        // TODO: add support for `eth_getHeaderByNumber`
        let block = geth_client.get_block_by_number((*block_num).into()).await?;
        println!(
            "=============>block_txs_len: {:?}",
            block.transactions.len()
        );
        println!("=============>block_hash: {:?}", block.hash);

        let circuit_config =
            crate::match_circuit_params!(block.gas_used.as_usize(), CIRCUIT_CONFIG, {
                return Err(format!(
                    "No circuit parameters found for block with gas used={}",
                    block.gas_used
                )
                .into());
            });
        log::info!("Using circuit config: {:#?}", circuit_config);

        println!("=============>circuit_config: {:?}", circuit_config);

        let circuit_params = CircuitsParams {
            max_txs: circuit_config.max_txs,
            max_calldata: circuit_config.max_calldata,
            max_bytecode: circuit_config.max_bytecode,
            max_rws: circuit_config.max_rws,
            max_copy_rows: circuit_config.max_copy_rows,
            max_inner_blocks: circuit_config.max_inner_blocks,
            max_exp_steps: circuit_config.max_exp_steps,
            max_evm_rows: circuit_config.pad_to,
            max_keccak_rows: circuit_config.keccak_padding,
        };
        let builder = BuilderClient::new(geth_client, circuit_params).await?;
        println!("=============>circuit_params: {:?}", circuit_params);

        let (builder, eth_block) = builder.gen_inputs(*block_num).await?;
        println!("=============>eth_block: {:?}", eth_block);

        Ok(Self {
            circuit_config,
            eth_block,
            block: builder.block,
            code_db: builder.code_db,
        })
    }

    pub fn evm_witness(&self) -> zkevm_circuits::witness::Block<Fr> {
        let mut block =
            evm_circuit::witness::block_convert(&self.block, &self.code_db).expect("block_convert");
        block.exp_circuit_pad_to = self.circuit_config.pad_to;
        // fixed randomness used in PublicInput contract and SuperCircuit
        block.randomness = Fr::from(0x100);

        block
    }

    pub fn gas_used(&self) -> u64 {
        self.eth_block.gas_used.as_u64()
    }

    pub fn txs(&self) -> Vec<geth_types::Transaction> {
        let txs = self
            .eth_block
            .transactions
            .iter()
            .map(geth_types::Transaction::from)
            .collect();

        txs
    }
}
