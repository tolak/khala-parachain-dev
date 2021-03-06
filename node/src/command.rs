// Copyright (C) 2021 HashForest Technology Pte. Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{
    chain_spec,
    cli::{Cli, RelayChainCli, Subcommand},
};
use codec::Encode;
use cumulus_client_service::genesis::generate_genesis_block;
use cumulus_primitives_core::ParaId;
use log::info;
use polkadot_parachain::primitives::AccountIdConversion;
use sc_cli::{
    ChainSpec, CliConfiguration, DefaultConfigurationValues, ImportParams, KeystoreParams,
    NetworkParams, Result, RuntimeVersion, SharedParams, SubstrateCli,
};
use sc_service::{
    config::{BasePath, PrometheusConfig},
    TaskManager,
};
use sp_core::hexdisplay::HexDisplay;
use sp_runtime::traits::Block as BlockT;
use std::{collections::VecDeque, io::Write, net::SocketAddr};

use crate::service::{Block, new_partial};

#[cfg(feature = "phala-native")]
use crate::service::phala::RuntimeExecutor as PhalaParachainRuntimeExecutor;
#[cfg(feature = "khala-native")]
use crate::service::khala::RuntimeExecutor as KhalaParachainRuntimeExecutor;
#[cfg(feature = "rhala-native")]
use crate::service::rhala::RuntimeExecutor as RhalaParachainRuntimeExecutor;
#[cfg(feature = "thala-native")]
use crate::service::thala::RuntimeExecutor as ThalaParachainRuntimeExecutor;
#[cfg(feature = "shell-native")]
use crate::service::shell::RuntimeExecutor as ShellParachainRuntimeExecutor;

trait IdentifyChain {
    fn runtime_name(&self) -> String;
    fn is_phala(&self) -> bool;
    fn is_khala(&self) -> bool;
    fn is_rhala(&self) -> bool;
    fn is_thala(&self) -> bool;
    fn is_shell(&self) -> bool;
}

impl IdentifyChain for dyn sc_service::ChainSpec {
    fn runtime_name(&self) -> String {
        chain_spec::Extensions::try_get(self)
            .map(|e| e.runtime.clone())
            .expect("Could not find parachain extension for chain-spec.")
    }
    fn is_phala(&self) -> bool {
        self.runtime_name() == "phala"
    }
    fn is_khala(&self) -> bool {
        self.runtime_name() == "khala"
    }
    fn is_rhala(&self) -> bool {
        self.runtime_name() == "rhala"
    }
    fn is_thala(&self) -> bool {
        self.runtime_name() == "thala"
    }
    fn is_shell(&self) -> bool {
        self.runtime_name() == "shell"
    }
}

impl<T: sc_service::ChainSpec + 'static> IdentifyChain for T {
    fn runtime_name(&self) -> String {
        <dyn sc_service::ChainSpec>::runtime_name(self)
    }
    fn is_phala(&self) -> bool {
        <dyn sc_service::ChainSpec>::is_phala(self)
    }
    fn is_khala(&self) -> bool {
        <dyn sc_service::ChainSpec>::is_khala(self)
    }
    fn is_rhala(&self) -> bool {
        <dyn sc_service::ChainSpec>::is_rhala(self)
    }
    fn is_thala(&self) -> bool {
        <dyn sc_service::ChainSpec>::is_thala(self)
    }
    fn is_shell(&self) -> bool {
        <dyn sc_service::ChainSpec>::is_shell(self)
    }
}

fn load_spec(id: &str) -> std::result::Result<Box<dyn sc_service::ChainSpec>, String> {
    let path = std::path::PathBuf::from(id);
    if id.to_lowercase().ends_with(".json") && path.exists() {
        info!("Load chain spec {}", path.to_str().unwrap());
        let chain_spec =
            chain_spec::ChainSpec::from_json_file(path.clone().into())?;
        let parsed: Box<dyn sc_service::ChainSpec> = match chain_spec.runtime_name().as_str() {
            #[cfg(feature = "phala-native")]
            "phala" => Box::new(chain_spec::phala::ChainSpec::from_json_file(path.into())?),
            #[cfg(feature = "khala-native")]
            "khala" => Box::new(chain_spec::khala::ChainSpec::from_json_file(path.into())?),
            #[cfg(feature = "rhala-native")]
            "rhala" => Box::new(chain_spec::rhala::ChainSpec::from_json_file(path.into())?),
            #[cfg(feature = "thala-native")]
            "thala" => Box::new(chain_spec::thala::ChainSpec::from_json_file(path.into())?),
            #[cfg(feature = "shell-native")]
            "shell" => Box::new(chain_spec::shell::ChainSpec::from_json_file(path.into())?),
            _ => return Err("`chain` must starts with a known runtime name!".to_string()),
        };
        return Ok(parsed);
    }

    let mut normalized_id: VecDeque<&str> = id.split("-").collect();
    if normalized_id.len() > 3 {
        return Err(
            "ParaId pattern must be runtime_name-profile-para_id or runtime_name-para_id"
                .into(),
        );
    }

    let runtime_name = normalized_id.pop_front().expect("Never empty");
    let profile = normalized_id.pop_front().ok_or("Profile skipped");
    let para_id = normalized_id
        .pop_front()
        .map(|id| id.parse::<u32>().or(Err("No parachain id")))
        .transpose()?
        .ok_or("Must specify parachain id");
    drop(normalized_id);

    info!(
        "Load native runtime: {}, profile: {}, para-id: {}",
        runtime_name,
        profile.unwrap_or("(Not Provide)"),
        para_id.unwrap_or(0)
    );

    #[cfg(feature = "phala-native")]
    if runtime_name == "phala" {
        if profile.is_err() && para_id.is_err() {
            return Ok(Box::new(chain_spec::phala::ChainSpec::from_json_bytes(
                &include_bytes!("../res/phala.json")[..],
            )?));
        }

        return match profile? {
            "dev" => Ok(Box::new(chain_spec::phala::development_config(
                para_id?.into(),
            ))),
            "local" => Ok(Box::new(chain_spec::phala::local_config(
                para_id?.into(),
            ))),
            "staging" => Ok(Box::new(chain_spec::phala::staging_config())),
            other => Err(format!("Unknown profile {} for Phala", other)),
        };
    }
    #[cfg(not(feature = "phala-native"))]
    if runtime_name == "phala" {
        return Err(format!("`{}` only supported with `phala-native` feature enabled.", id))
    }

    #[cfg(feature = "khala-native")]
    if runtime_name == "khala" {
        if profile.is_err() && para_id.is_err() {
            return Ok(Box::new(chain_spec::khala::ChainSpec::from_json_bytes(
                &include_bytes!("../res/khala.json")[..],
            )?));
        }

        return match profile? {
            "dev" => Ok(Box::new(chain_spec::khala::development_config(
                para_id?.into(),
            ))),
            "local" => Ok(Box::new(chain_spec::khala::local_config(
                para_id?.into(),
            ))),
            "staging" => Ok(Box::new(chain_spec::khala::staging_config())),
            other => Err(format!("Unknown profile {} for Khala", other)),
        };
    }
    #[cfg(not(feature = "khala-native"))]
    if runtime_name == "khala" {
        return Err(format!("`{}` only supported with `khala-native` feature enabled.", id))
    }

    #[cfg(feature = "rhala-native")]
    if runtime_name == "rhala" {
        // TODO: Export when we preparing for Rococo
        // if profile.is_err() && para_id.is_err() {
        //     return Ok(Box::new(chain_spec::rhala::ChainSpec::from_json_bytes(
        //         &include_bytes!("../res/rhala.json")[..],
        //     )?));
        // }

        return match profile? {
            "dev" => Ok(Box::new(chain_spec::rhala::development_config(
                para_id?.into(),
            ))),
            "local" => Ok(Box::new(chain_spec::rhala::local_config(
                para_id?.into(),
            ))),
            "staging" => Ok(Box::new(chain_spec::rhala::staging_config())),
            other => Err(format!("Unknown profile {} for Rhala", other)),
        };
    }
    #[cfg(not(feature = "rhala-native"))]
    if runtime_name == "rhala" {
        return Err(format!("`{}` only supported with `rhala-native` feature enabled.", id))
    }

    #[cfg(feature = "thala-native")]
    if runtime_name == "thala" {
        return match profile? {
            "dev" => Ok(Box::new(chain_spec::thala::development_config(
                para_id?.into(),
            ))),
            "local" => Ok(Box::new(chain_spec::thala::local_config(para_id?.into()))),
            other => Err(format!("Unknown profile {} for Thala", other)),
        };
    }
    #[cfg(not(feature = "thala-native"))]
    if runtime_name == "thala" {
        return Err(format!("`{}` only supported with `thala-native` feature enabled.", id))
    }

    #[cfg(feature = "shell-native")]
    if runtime_name == "shell" {
        if profile.is_err() && para_id.is_err() {
            return Ok(Box::new(chain_spec::shell::ChainSpec::from_json_bytes(
                &include_bytes!("../res/shell.json")[..],
            )?));
        }

        return match profile? {
            "dev" => Ok(Box::new(chain_spec::shell::development_config(para_id?.into()))),
            "staging" => Ok(Box::new(chain_spec::shell::staging_config())),
            other => Err(format!("Unknown profile {} for Shell", other)),
        };
    }
    #[cfg(not(feature = "shell-native"))]
    if runtime_name == "shell" {
        return Err(format!("`{}` only supported with `shell-native` feature enabled.", id))
    }

    Err(format!(
        "Invalid `--chain` arg. \
            give exported chain-spec or follow pattern: runtime-profile-para_id"
    ))
}

impl SubstrateCli for Cli {
    fn impl_name() -> String {
        "Khala Node".into()
    }

    fn impl_version() -> String {
        env!("SUBSTRATE_CLI_IMPL_VERSION").into()
    }

    fn description() -> String {
        format!(
            "Khala Node\n\nThe command-line arguments provided first will be \
            passed to the parachain node, while the arguments provided after -- will be passed \
            to the relaychain node.\n\n\
            {} [parachain-args] -- [relaychain-args]",
            Self::executable_name()
        )
    }

    fn author() -> String {
        env!("CARGO_PKG_AUTHORS").into()
    }

    fn support_url() -> String {
        "https://github.com/Phala-Network/khala-parachain/issues/new".into()
    }

    fn copyright_start_year() -> i32 {
        2018
    }

    fn load_spec(&self, id: &str) -> std::result::Result<Box<dyn sc_service::ChainSpec>, String> {
        load_spec(id)
    }

    fn native_runtime_version(_: &Box<dyn ChainSpec>) -> &'static RuntimeVersion {
        &khala_parachain_runtime::VERSION
    }
}

impl SubstrateCli for RelayChainCli {
    fn impl_name() -> String {
        "Khala Node".into()
    }

    fn impl_version() -> String {
        env!("SUBSTRATE_CLI_IMPL_VERSION").into()
    }

    fn description() -> String {
        "Khala Node\n\nThe command-line arguments provided first will be \
        passed to the parachain node, while the arguments provided after -- will be passed \
        to the relaychain node.\n\n\
        khala [parachain-args] -- [relaychain-args]"
            .into()
    }

    fn author() -> String {
        env!("CARGO_PKG_AUTHORS").into()
    }

    fn support_url() -> String {
        "https://github.com/Phala-Network/khala-parachain/issues/new".into()
    }

    fn copyright_start_year() -> i32 {
        2018
    }

    fn load_spec(&self, id: &str) -> std::result::Result<Box<dyn sc_service::ChainSpec>, String> {
        polkadot_cli::Cli::from_iter([RelayChainCli::executable_name().to_string()].iter())
            .load_spec(id)
    }

    fn native_runtime_version(chain_spec: &Box<dyn ChainSpec>) -> &'static RuntimeVersion {
        polkadot_cli::Cli::native_runtime_version(chain_spec)
    }
}

fn extract_genesis_wasm(chain_spec: &Box<dyn sc_service::ChainSpec>) -> Result<Vec<u8>> {
    let mut storage = chain_spec.build_storage()?;

    storage
        .top
        .remove(sp_core::storage::well_known_keys::CODE)
        .ok_or_else(|| "Could not find wasm file in genesis state!".into())
}

macro_rules! construct_async_run {
    (|$components:ident, $cli:ident, $cmd:ident, $config:ident| $( $code:tt )* ) => {{
        let runner = $cli.create_runner($cmd)?;

        #[cfg(feature = "phala-native")]
        if runner.config().chain_spec.is_phala() {
            return runner.async_run(|$config| {
                let $components = new_partial::<phala_parachain_runtime::RuntimeApi, PhalaParachainRuntimeExecutor, _>(
                    &$config,
                    crate::service::phala::parachain_build_import_queue,
                )?;
                let task_manager = $components.task_manager;
                { $( $code )* }.map(|v| (v, task_manager))
            })
        }

        #[cfg(feature = "khala-native")]
        if runner.config().chain_spec.is_khala() {
            return runner.async_run(|$config| {
                let $components = new_partial::<khala_parachain_runtime::RuntimeApi, KhalaParachainRuntimeExecutor, _>(
                    &$config,
                    crate::service::khala::parachain_build_import_queue,
                )?;
                let task_manager = $components.task_manager;
                { $( $code )* }.map(|v| (v, task_manager))
            })
        }

        #[cfg(feature = "rhala-native")]
        if runner.config().chain_spec.is_rhala() {
            return runner.async_run(|$config| {
                let $components = new_partial::<rhala_parachain_runtime::RuntimeApi, RhalaParachainRuntimeExecutor, _>(
                    &$config,
                    crate::service::rhala::parachain_build_import_queue,
                )?;
                let task_manager = $components.task_manager;
                { $( $code )* }.map(|v| (v, task_manager))
            })
        }

        #[cfg(feature = "thala-native")]
        if runner.config().chain_spec.is_thala() {
            return runner.async_run(|$config| {
                let $components = new_partial::<thala_parachain_runtime::RuntimeApi, ThalaParachainRuntimeExecutor, _>(
                    &$config,
                    crate::service::thala::parachain_build_import_queue,
                )?;
                let task_manager = $components.task_manager;
                { $( $code )* }.map(|v| (v, task_manager))
            })
        }

        #[cfg(feature = "shell-native")]
        if runner.config().chain_spec.is_shell() {
            return runner.async_run(|$config| {
                let $components = new_partial::<shell_parachain_runtime::RuntimeApi, ShellParachainRuntimeExecutor, _>(
                    &$config,
                    crate::service::shell::parachain_build_import_queue,
                )?;
                let task_manager = $components.task_manager;
                { $( $code )* }.map(|v| (v, task_manager))
            })
        }

        panic!("Can not determine runtime")
    }}
}

/// Parse command line arguments into service configuration.
pub fn run() -> Result<()> {
    let cli = Cli::from_args();

    match &cli.subcommand {
        Some(Subcommand::BuildSpec(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run(config.chain_spec, config.network))
        }
        Some(Subcommand::CheckBlock(cmd)) => {
            construct_async_run!(|components, cli, cmd, config| {
                Ok(cmd.run(components.client, components.import_queue))
            })
        }
        Some(Subcommand::ExportBlocks(cmd)) => {
            construct_async_run!(|components, cli, cmd, config| {
                Ok(cmd.run(components.client, config.database))
            })
        }
        Some(Subcommand::ExportState(cmd)) => {
            construct_async_run!(|components, cli, cmd, config| {
                Ok(cmd.run(components.client, config.chain_spec))
            })
        }
        Some(Subcommand::ImportBlocks(cmd)) => {
            construct_async_run!(|components, cli, cmd, config| {
                Ok(cmd.run(components.client, components.import_queue))
            })
        }
        Some(Subcommand::PurgeChain(cmd)) => {
            let runner = cli.create_runner(cmd)?;

            runner.sync_run(|config| {
                let polkadot_cli = RelayChainCli::new(
                    &config,
                    [RelayChainCli::executable_name().to_string()]
                        .iter()
                        .chain(cli.relay_chain_args.iter()),
                );

                let polkadot_config = SubstrateCli::create_configuration(
                    &polkadot_cli,
                    &polkadot_cli,
                    config.tokio_handle.clone(),
                )
                .map_err(|err| format!("Relay chain argument error: {}", err))?;

                cmd.run(config, polkadot_config)
            })
        }
        Some(Subcommand::Revert(cmd)) => construct_async_run!(|components, cli, cmd, config| {
            Ok(cmd.run(components.client, components.backend))
        }),
        Some(Subcommand::ExportGenesisState(params)) => {
            let mut builder = sc_cli::LoggerBuilder::new("");
            builder.with_profiling(sc_tracing::TracingReceiver::Log, "");
            let _ = builder.init();

            let spec = load_spec(&params.chain.clone().unwrap_or_default())?;
            let state_version = Cli::native_runtime_version(&spec).state_version();
            let block: Block = generate_genesis_block(&spec, state_version)?;
            let raw_header = block.header().encode();
            let output_buf = if params.raw {
                raw_header
            } else {
                format!("0x{:?}", HexDisplay::from(&block.header().encode())).into_bytes()
            };

            if let Some(output) = &params.output {
                std::fs::write(output, output_buf)?;
            } else {
                std::io::stdout().write_all(&output_buf)?;
            }

            Ok(())
        }
        Some(Subcommand::ExportGenesisWasm(params)) => {
            let mut builder = sc_cli::LoggerBuilder::new("");
            builder.with_profiling(sc_tracing::TracingReceiver::Log, "");
            let _ = builder.init();

            let raw_wasm_blob =
                extract_genesis_wasm(&cli.load_spec(&params.chain.clone().unwrap_or_default())?)?;
            let output_buf = if params.raw {
                raw_wasm_blob
            } else {
                format!("0x{:?}", HexDisplay::from(&raw_wasm_blob)).into_bytes()
            };

            if let Some(output) = &params.output {
                std::fs::write(output, output_buf)?;
            } else {
                std::io::stdout().write_all(&output_buf)?;
            }

            Ok(())
        }
        Some(Subcommand::Benchmark(cmd)) => {
            if cfg!(feature = "runtime-benchmarks") {
                let runner = cli.create_runner(cmd)?;

                #[cfg(feature = "phala-native")]
                if runner.config().chain_spec.is_phala() {
                    return runner.sync_run(|config| cmd.run::<Block, PhalaParachainRuntimeExecutor>(config))
                }

                #[cfg(feature = "khala-native")]
                if runner.config().chain_spec.is_khala() {
                    return runner.sync_run(|config| cmd.run::<Block, KhalaParachainRuntimeExecutor>(config))
                }

                #[cfg(feature = "rhala-native")]
                if runner.config().chain_spec.is_rhala() {
                    return runner.sync_run(|config| cmd.run::<Block, RhalaParachainRuntimeExecutor>(config))
                }

                #[cfg(feature = "thala-native")]
                if runner.config().chain_spec.is_thala() {
                    return runner.sync_run(|config| cmd.run::<Block, ThalaParachainRuntimeExecutor>(config))
                }

                Err("Chain doesn't support benchmarking".into())
            } else {
                Err("Benchmarking wasn't enabled when building the node. \
                    You can enable it with `--features runtime-benchmarks`."
                    .into())
            }
        }
        Some(Subcommand::Key(cmd)) => Ok(cmd.run(&cli)?),
        Some(Subcommand::TryRuntime(cmd)) => {
            if cfg!(feature = "try-runtime") {
                // grab the task manager.
                let runner = cli.create_runner(cmd)?;
                let registry = &runner.config().prometheus_config.as_ref().map(|cfg| &cfg.registry);
                let task_manager =
                    TaskManager::new(runner.config().tokio_handle.clone(), *registry)
                        .map_err(|e| format!("Error: {:?}", e))?;

                #[cfg(feature = "phala-native")]
                if runner.config().chain_spec.is_phala() {
                    return runner.async_run(|config| {
                        Ok((cmd.run::<Block, PhalaParachainRuntimeExecutor>(config), task_manager))
                    })
                }

                #[cfg(feature = "khala-native")]
                if runner.config().chain_spec.is_khala() {
                    return runner.async_run(|config| {
                        Ok((cmd.run::<Block, KhalaParachainRuntimeExecutor>(config), task_manager))
                    })
                }

                #[cfg(feature = "rhala-native")]
                if runner.config().chain_spec.is_rhala() {
                    return runner.async_run(|config| {
                        Ok((cmd.run::<Block, RhalaParachainRuntimeExecutor>(config), task_manager))
                    })
                }

                #[cfg(feature = "thala-native")]
                if runner.config().chain_spec.is_thala() {
                    return runner.async_run(|config| {
                        Ok((cmd.run::<Block, ThalaParachainRuntimeExecutor>(config), task_manager))
                    })
                }

                #[cfg(feature = "shell-native")]
                if runner.config().chain_spec.is_shell() {
                    return runner.async_run(|config| {
                        Ok((cmd.run::<Block, ShellParachainRuntimeExecutor>(config), task_manager))
                    })
                }

                Err("Can't determine runtime from chain_spec".into())
            } else {
                Err("Try-runtime must be enabled by `--features try-runtime`.".into())
            }
        },
        None => {
            let runner = cli.create_runner(&cli.run.normalize())?;

            runner.run_node_until_exit(|config| async move {
                let para_id =
                    chain_spec::Extensions::try_get(&*config.chain_spec)
                        .map(|e| e.para_id)
                        .ok_or_else(|| "Could not find parachain extension for chain-spec.")?;

                let polkadot_cli = RelayChainCli::new(
                    &config,
                    [RelayChainCli::executable_name().to_string()]
                        .iter()
                        .chain(cli.relay_chain_args.iter()),
                );

                let id = ParaId::from(para_id);

                let parachain_account =
                    AccountIdConversion::<polkadot_primitives::v0::AccountId>::into_account(&id);

                let state_version =
                    RelayChainCli::native_runtime_version(&config.chain_spec).state_version();
                let block: Block = generate_genesis_block(&config.chain_spec, state_version)
                    .map_err(|e| format!("{:?}", e))?;
                let genesis_state = format!("0x{:?}", HexDisplay::from(&block.header().encode()));

                let task_executor = config.tokio_handle.clone();
                let polkadot_config =
                    SubstrateCli::create_configuration(&polkadot_cli, &polkadot_cli, task_executor)
                        .map_err(|err| format!("Relay chain argument error: {}", err))?;

                info!("Parachain id: {:?}", id);
                info!("Parachain Account: {}", parachain_account);
                info!("Parachain genesis state: {}", genesis_state);
                info!(
                    "Is collating: {}",
                    if config.role.is_authority() {
                        "yes"
                    } else {
                        "no"
                    }
                );

                #[cfg(feature = "phala-native")]
                if config.chain_spec.is_phala() {
                    return crate::service::phala::start_parachain_node(config, polkadot_config, id)
                        .await
                        .map(|r| r.0)
                        .map_err(Into::into)
                }

                #[cfg(feature = "khala-native")]
                if config.chain_spec.is_khala() {
                    return crate::service::khala::start_parachain_node(config, polkadot_config, id)
                        .await
                        .map(|r| r.0)
                        .map_err(Into::into)
                }

                #[cfg(feature = "rhala-native")]
                if config.chain_spec.is_rhala() {
                    return crate::service::rhala::start_parachain_node(config, polkadot_config, id)
                        .await
                        .map(|r| r.0)
                        .map_err(Into::into)
                }

                #[cfg(feature = "thala-native")]
                if config.chain_spec.is_thala() {
                    return crate::service::thala::start_parachain_node(config, polkadot_config, id)
                        .await
                        .map(|r| r.0)
                        .map_err(Into::into)
                }

                #[cfg(feature = "shell-native")]
                if config.chain_spec.is_shell() {
                    return crate::service::shell::start_parachain_node(config, polkadot_config, id)
                        .await
                        .map(|r| r.0)
                        .map_err(Into::into)
                }

                Err("Can't determine runtime from chain_spec".into())
            })
        }
    }
}

impl DefaultConfigurationValues for RelayChainCli {
    fn p2p_listen_port() -> u16 {
        30334
    }

    fn rpc_ws_listen_port() -> u16 {
        9945
    }

    fn rpc_http_listen_port() -> u16 {
        9934
    }

    fn prometheus_listen_port() -> u16 {
        9616
    }
}

impl CliConfiguration<Self> for RelayChainCli {
    fn shared_params(&self) -> &SharedParams {
        self.base.base.shared_params()
    }

    fn import_params(&self) -> Option<&ImportParams> {
        self.base.base.import_params()
    }

    fn network_params(&self) -> Option<&NetworkParams> {
        self.base.base.network_params()
    }

    fn keystore_params(&self) -> Option<&KeystoreParams> {
        self.base.base.keystore_params()
    }

    fn base_path(&self) -> Result<Option<BasePath>> {
        Ok(self
            .shared_params()
            .base_path()
            .or_else(|| self.base_path.clone().map(Into::into)))
    }

    fn rpc_http(&self, default_listen_port: u16) -> Result<Option<SocketAddr>> {
        self.base.base.rpc_http(default_listen_port)
    }

    fn rpc_ipc(&self) -> Result<Option<String>> {
        self.base.base.rpc_ipc()
    }

    fn rpc_ws(&self, default_listen_port: u16) -> Result<Option<SocketAddr>> {
        self.base.base.rpc_ws(default_listen_port)
    }

    fn prometheus_config(
        &self,
        default_listen_port: u16,
        chain_spec: &Box<dyn ChainSpec>,
    ) -> Result<Option<PrometheusConfig>> {
        self.base.base.prometheus_config(default_listen_port, chain_spec)
    }

    fn init<F>(
        &self,
        _support_url: &String,
        _impl_version: &String,
        _logger_hook: F,
        _config: &sc_service::Configuration,
    ) -> Result<()>
        where
            F: FnOnce(&mut sc_cli::LoggerBuilder, &sc_service::Configuration),
    {
        unreachable!("PolkadotCli is never initialized; qed");
    }

    fn chain_id(&self, is_dev: bool) -> Result<String> {
        let chain_id = self.base.base.chain_id(is_dev)?;

        Ok(if chain_id.is_empty() { self.chain_id.clone().unwrap_or_default() } else { chain_id })
    }

    fn role(&self, is_dev: bool) -> Result<sc_service::Role> {
        self.base.base.role(is_dev)
    }

    fn transaction_pool(&self) -> Result<sc_service::config::TransactionPoolOptions> {
        self.base.base.transaction_pool()
    }

    fn state_cache_child_ratio(&self) -> Result<Option<usize>> {
        self.base.base.state_cache_child_ratio()
    }

    fn rpc_methods(&self) -> Result<sc_service::config::RpcMethods> {
        self.base.base.rpc_methods()
    }

    fn rpc_ws_max_connections(&self) -> Result<Option<usize>> {
        self.base.base.rpc_ws_max_connections()
    }

    fn rpc_cors(&self, is_dev: bool) -> Result<Option<Vec<String>>> {
        self.base.base.rpc_cors(is_dev)
    }

    fn default_heap_pages(&self) -> Result<Option<u64>> {
        self.base.base.default_heap_pages()
    }

    fn force_authoring(&self) -> Result<bool> {
        self.base.base.force_authoring()
    }

    fn disable_grandpa(&self) -> Result<bool> {
        self.base.base.disable_grandpa()
    }

    fn max_runtime_instances(&self) -> Result<Option<usize>> {
        self.base.base.max_runtime_instances()
    }

    fn announce_block(&self) -> Result<bool> {
        self.base.base.announce_block()
    }

    fn telemetry_endpoints(
        &self,
        chain_spec: &Box<dyn ChainSpec>,
    ) -> Result<Option<sc_telemetry::TelemetryEndpoints>> {
        self.base.base.telemetry_endpoints(chain_spec)
    }

    fn node_name(&self) -> Result<String> {
        self.base.base.node_name()
    }
}
