#![allow(clippy::type_complexity)]

use {
  self::{command_builder::CommandBuilder, expected::Expected, test_server::TestServer},
  bitcoin::{
    address::{Address, NetworkUnchecked},
    blockdata::constants::COIN_VALUE,
    Network, OutPoint, Txid, Witness,
  },
  bitcoincore_rpc::bitcoincore_rpc_json::ListDescriptorsResult,
  chrono::{DateTime, Utc},
  executable_path::executable_path,
  ord::{
    api, chain::Chain, outgoing::Outgoing, subcommand::runes::RuneInfo, wallet::batch, Edict,
    InscriptionId, Pile, Rune, RuneEntry, RuneId, Runestone, SpacedRune,
  },
  ordinals::{Charm, Rarity, Sat, SatPoint},
  pretty_assertions::assert_eq as pretty_assert_eq,
  regex::Regex,
  reqwest::{StatusCode, Url},
  serde::de::DeserializeOwned,
  std::sync::Arc,
  std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    str::{self, FromStr},
    thread,
    time::Duration,
  },
  tempfile::TempDir,
  test_bitcoincore_rpc::TransactionTemplate,
};

macro_rules! assert_regex_match {
  ($value:expr, $pattern:expr $(,)?) => {
    let regex = Regex::new(&format!("^(?s){}$", $pattern)).unwrap();
    let string = $value.to_string();

    if !regex.is_match(string.as_ref()) {
      eprintln!("Regex did not match:");
      pretty_assert_eq!(regex.as_str(), string);
    }
  };
}

mod command_builder;
mod expected;
mod test_server;

mod balances;
mod decode;
mod epochs;
mod find;
mod index;
mod info;
mod json_api;
mod list;
mod parse;
mod runes;
mod server;
mod settings;
mod subsidy;
mod supply;
mod traits;
mod version;
mod wallet;

const RUNE: u128 = 99246114928149462;

type Balance = ord::subcommand::wallet::balance::Output;
type Create = ord::subcommand::wallet::create::Output;
type Inscribe = ord::wallet::batch::Output;
type Inscriptions = Vec<ord::subcommand::wallet::inscriptions::Output>;
type Send = ord::subcommand::wallet::send::Output;
type Supply = ord::subcommand::supply::Output;

fn create_wallet(bitcoin_rpc_server: &test_bitcoincore_rpc::Handle, ord_rpc_server: &TestServer) {
  CommandBuilder::new(format!(
    "--chain {} wallet create",
    bitcoin_rpc_server.network()
  ))
  .bitcoin_rpc_server(bitcoin_rpc_server)
  .ord_rpc_server(ord_rpc_server)
  .run_and_deserialize_output::<ord::subcommand::wallet::create::Output>();
}

fn receive(
  bitcoin_rpc_server: &test_bitcoincore_rpc::Handle,
  ord_rpc_server: &TestServer,
) -> Address {
  let address = CommandBuilder::new("wallet receive")
    .bitcoin_rpc_server(bitcoin_rpc_server)
    .ord_rpc_server(ord_rpc_server)
    .run_and_deserialize_output::<ord::subcommand::wallet::receive::Output>()
    .addresses
    .into_iter()
    .next()
    .unwrap();

  address
    .require_network(bitcoin_rpc_server.state().network)
    .unwrap()
}

fn sats(
  bitcoin_rpc_server: &test_bitcoincore_rpc::Handle,
  ord_rpc_server: &TestServer,
) -> Vec<ord::subcommand::wallet::sats::OutputRare> {
  CommandBuilder::new(format!(
    "--chain {} wallet sats",
    bitcoin_rpc_server.network()
  ))
  .bitcoin_rpc_server(bitcoin_rpc_server)
  .ord_rpc_server(ord_rpc_server)
  .run_and_deserialize_output::<Vec<ord::subcommand::wallet::sats::OutputRare>>()
}

fn inscribe(
  bitcoin_rpc_server: &test_bitcoincore_rpc::Handle,
  ord_rpc_server: &TestServer,
) -> (InscriptionId, Txid) {
  bitcoin_rpc_server.mine_blocks(1);

  let output = CommandBuilder::new(format!(
    "--chain {} wallet inscribe --fee-rate 1 --file foo.txt",
    bitcoin_rpc_server.network()
  ))
  .write("foo.txt", "FOO")
  .bitcoin_rpc_server(bitcoin_rpc_server)
  .ord_rpc_server(ord_rpc_server)
  .run_and_deserialize_output::<Inscribe>();

  bitcoin_rpc_server.mine_blocks(1);

  assert_eq!(output.inscriptions.len(), 1);

  (output.inscriptions[0].id, output.reveal)
}

fn drain(bitcoin_rpc_server: &test_bitcoincore_rpc::Handle, ord_rpc_server: &TestServer) {
  let balance = CommandBuilder::new("--regtest --index-runes wallet balance")
    .bitcoin_rpc_server(bitcoin_rpc_server)
    .ord_rpc_server(ord_rpc_server)
    .run_and_deserialize_output::<Balance>();

  CommandBuilder::new(format!(
    "
      --chain regtest
      --index-runes
      wallet send
      --fee-rate 0
      bcrt1pyrmadgg78e38ewfv0an8c6eppk2fttv5vnuvz04yza60qau5va0saknu8k
      {}sat
    ",
    balance.cardinal
  ))
  .bitcoin_rpc_server(bitcoin_rpc_server)
  .ord_rpc_server(ord_rpc_server)
  .run_and_deserialize_output::<Send>();

  bitcoin_rpc_server.mine_blocks_with_subsidy(1, 0);

  let balance = CommandBuilder::new("--regtest --index-runes wallet balance")
    .bitcoin_rpc_server(bitcoin_rpc_server)
    .ord_rpc_server(ord_rpc_server)
    .run_and_deserialize_output::<Balance>();

  pretty_assert_eq!(balance.cardinal, 0);
}

struct Etched {
  id: RuneId,
  inscribe: Inscribe,
}

fn etch(
  bitcoin_rpc_server: &test_bitcoincore_rpc::Handle,
  ord_rpc_server: &TestServer,
  rune: Rune,
) -> Etched {
  batch(
    bitcoin_rpc_server,
    ord_rpc_server,
    batch::File {
      etching: Some(batch::Etching {
        supply: "1000".parse().unwrap(),
        divisibility: 0,
        terms: None,
        premine: "1000".parse().unwrap(),
        rune: SpacedRune { rune, spacers: 0 },
        symbol: '¢',
      }),
      inscriptions: vec![batch::Entry {
        file: "inscription.jpeg".into(),
        ..default()
      }],
      ..default()
    },
  )
}

fn batch(
  bitcoin_rpc_server: &test_bitcoincore_rpc::Handle,
  ord_rpc_server: &TestServer,
  batchfile: batch::File,
) -> Etched {
  bitcoin_rpc_server.mine_blocks(1);

  let mut builder =
    CommandBuilder::new("--regtest --index-runes wallet inscribe --fee-rate 0 --batch batch.yaml")
      .write("batch.yaml", serde_yaml::to_string(&batchfile).unwrap())
      .bitcoin_rpc_server(bitcoin_rpc_server)
      .ord_rpc_server(ord_rpc_server);

  for inscription in &batchfile.inscriptions {
    builder = builder.write(&inscription.file, "inscription");
  }

  let mut spawn = builder.spawn();

  let mut buffer = String::new();

  BufReader::new(spawn.child.stderr.as_mut().unwrap())
    .read_line(&mut buffer)
    .unwrap();

  assert_eq!(buffer, "Waiting for rune commitment to mature…\n");

  bitcoin_rpc_server.mine_blocks(6);

  let inscribe = spawn.run_and_deserialize_output::<Inscribe>();

  bitcoin_rpc_server.mine_blocks(1);

  let block_height = bitcoin_rpc_server.height();

  let id = RuneId {
    block: block_height,
    tx: 1,
  };

  let reveal = inscribe.reveal;
  let parent = inscribe.inscriptions[0].id;

  let batch::Etching {
    divisibility,
    premine,
    rune,
    supply,
    symbol,
    terms,
  } = batchfile.etching.unwrap();

  {
    let supply = supply.to_integer(divisibility).unwrap();
    let premine = premine.to_integer(divisibility).unwrap();

    let mintable = terms
      .map(|terms| terms.cap * terms.amount.to_integer(divisibility).unwrap())
      .unwrap_or_default();

    assert_eq!(supply, premine + mintable);
  }

  let mut mint_definition = Vec::<String>::new();

  if let Some(terms) = terms {
    mint_definition.push("<dd>".into());
    mint_definition.push("<dl>".into());

    let mut mintable = true;

    mint_definition.push("<dt>start</dt>".into());
    {
      let relative = terms
        .offset
        .and_then(|range| range.start)
        .map(|start| start + block_height);
      let absolute = terms.height.and_then(|range| range.start);

      let start = relative
        .zip(absolute)
        .map(|(relative, absolute)| relative.max(absolute))
        .or(relative)
        .or(absolute);

      if let Some(start) = start {
        mintable &= block_height + 1 >= start;
        mint_definition.push(format!("<dd><a href=/block/{start}>{start}</a></dd>"));
      } else {
        mint_definition.push("<dd>none</dd>".into());
      }
    }

    mint_definition.push("<dt>end</dt>".into());
    {
      let relative = terms
        .offset
        .and_then(|range| range.end)
        .map(|end| end + block_height);
      let absolute = terms.height.and_then(|range| range.end);

      let end = relative
        .zip(absolute)
        .map(|(relative, absolute)| relative.min(absolute))
        .or(relative)
        .or(absolute);

      if let Some(end) = end {
        mintable &= block_height + 1 < end;
        mint_definition.push(format!("<dd><a href=/block/{end}>{end}</a></dd>"));
      } else {
        mint_definition.push("<dd>none</dd>".into());
      }
    }

    mint_definition.push("<dt>amount</dt>".into());

    mint_definition.push(format!(
      "<dd>{}</dd>",
      Pile {
        amount: terms.amount.to_integer(divisibility).unwrap(),
        divisibility,
        symbol: Some(symbol),
      }
    ));

    mint_definition.push("<dt>mints</dt>".into());
    mint_definition.push("<dd>0</dd>".into());
    mint_definition.push("<dt>cap</dt>".into());
    mint_definition.push(format!("<dd>{}</dd>", terms.cap));
    mint_definition.push("<dt>remaining</dt>".into());
    mint_definition.push(format!("<dd>{}</dd>", terms.cap));

    mint_definition.push("<dt>mintable</dt>".into());
    mint_definition.push(format!("<dd>{mintable}</dd>"));

    mint_definition.push("</dl>".into());
    mint_definition.push("</dd>".into());
  } else {
    mint_definition.push("<dd>no</dd>".into());
  }

  let RuneId { block, tx } = id;

  ord_rpc_server.assert_response_regex(
    format!("/rune/{rune}"),
    format!(
      r".*<dt>id</dt>
  <dd>{id}</dd>.*
  <dt>etching block</dt>
  <dd><a href=/block/{block}>{block}</a></dd>
  <dt>etching transaction</dt>
  <dd>{tx}</dd>
  <dt>mint</dt>
  {}
  <dt>supply</dt>
  <dd>{premine} {symbol}</dd>
  <dt>premine</dt>
  <dd>{premine} {symbol}</dd>
  <dt>burned</dt>
  <dd>0 {symbol}</dd>
  <dt>divisibility</dt>
  <dd>{divisibility}</dd>
  <dt>symbol</dt>
  <dd>{symbol}</dd>
  <dt>etching</dt>
  <dd><a class=monospace href=/tx/{reveal}>{reveal}</a></dd>
  <dt>parent</dt>
  <dd><a class=monospace href=/inscription/{parent}>{parent}</a></dd>
.*",
      mint_definition.join("\\s+"),
    ),
  );

  let batch::RuneInfo {
    destination,
    location,
    rune,
  } = inscribe.rune.clone().unwrap();

  if premine.to_integer(divisibility).unwrap() > 0 {
    let destination = destination
      .unwrap()
      .clone()
      .require_network(Network::Regtest)
      .unwrap();

    assert!(bitcoin_rpc_server.state().is_wallet_address(&destination));

    let location = location.unwrap();

    ord_rpc_server.assert_response_regex(
      "/runes/balances",
      format!(
        ".*<tr>
    <td><a href=/rune/{rune}>{rune}</a></td>
    <td>
      <table>
        <tr>
          <td class=monospace>
            <a href=/output/{location}>{location}</a>
          </td>
          <td class=monospace>
            {premine}\u{A0}{symbol}
          </td>
        </tr>
      </table>
    </td>
  </tr>.*"
      ),
    );

    assert_eq!(bitcoin_rpc_server.address(location), destination);
  } else {
    assert!(destination.is_none());
    assert!(location.is_none());
  }

  let response = ord_rpc_server.json_request("/inscriptions");

  assert!(response.status().is_success());

  for id in response.json::<api::Inscriptions>().unwrap().ids {
    let response = ord_rpc_server.json_request(format!("/inscription/{id}"));
    assert!(response.status().is_success());
    if let Some(location) = location {
      let inscription = response.json::<api::Inscription>().unwrap();
      assert!(inscription.satpoint.outpoint != location);
    }
  }

  Etched { inscribe, id }
}

fn envelope(payload: &[&[u8]]) -> Witness {
  let mut builder = bitcoin::script::Builder::new()
    .push_opcode(bitcoin::opcodes::OP_FALSE)
    .push_opcode(bitcoin::opcodes::all::OP_IF);

  for data in payload {
    let mut buf = bitcoin::script::PushBytesBuf::new();
    buf.extend_from_slice(data).unwrap();
    builder = builder.push_slice(buf);
  }

  let script = builder
    .push_opcode(bitcoin::opcodes::all::OP_ENDIF)
    .into_script();

  Witness::from_slice(&[script.into_bytes(), Vec::new()])
}

fn default<T: Default>() -> T {
  Default::default()
}
