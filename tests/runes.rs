use {super::*, ord::subcommand::runes::Output};

#[test]
fn flag_is_required() {
  let bitcoin_rpc_server = test_bitcoincore_rpc::builder()
    .network(Network::Regtest)
    .build();

  let ord_rpc_server = TestServer::spawn_with_server_args(&bitcoin_rpc_server, &["--regtest"], &[]);

  CommandBuilder::new("--regtest runes")
    .bitcoin_rpc_server(&bitcoin_rpc_server)
    .ord_rpc_server(&ord_rpc_server)
    .expected_exit_code(1)
    .expected_stderr("error: `ord runes` requires index created with `--index-runes` flag\n")
    .run_and_extract_stdout();
}

#[test]
fn no_runes() {
  let bitcoin_rpc_server = test_bitcoincore_rpc::builder()
    .network(Network::Regtest)
    .build();

  assert_eq!(
    CommandBuilder::new("--index-runes --regtest runes")
      .bitcoin_rpc_server(&bitcoin_rpc_server)
      .run_and_deserialize_output::<Output>(),
    Output {
      runes: BTreeMap::new(),
    }
  );
}

#[test]
fn one_rune() {
  let bitcoin_rpc_server = test_bitcoincore_rpc::builder()
    .network(Network::Regtest)
    .build();

  let ord_rpc_server =
    TestServer::spawn_with_server_args(&bitcoin_rpc_server, &["--regtest", "--index-runes"], &[]);

  create_wallet(&bitcoin_rpc_server, &ord_rpc_server);

  let etch = etch(&bitcoin_rpc_server, &ord_rpc_server, Rune(RUNE));

  pretty_assert_eq!(
    CommandBuilder::new("--index-runes --regtest runes")
      .bitcoin_rpc_server(&bitcoin_rpc_server)
      .run_and_deserialize_output::<Output>(),
    Output {
      runes: vec![(
        Rune(RUNE),
        RuneInfo {
          block: 8,
          burned: 0,
          divisibility: 0,
          etching: etch.inscribe.reveal,
          id: RuneId { block: 8, tx: 1 },
          terms: None,
          mints: 0,
          number: 0,
          premine: 1000,
          rune: SpacedRune {
            rune: Rune(RUNE),
            spacers: 0
          },
          supply: 1000,
          symbol: Some('¢'),
          timestamp: ord::timestamp(8),
          tx: 1,
        }
      )]
      .into_iter()
      .collect(),
    }
  );
}

#[test]
fn two_runes() {
  let bitcoin_rpc_server = test_bitcoincore_rpc::builder()
    .network(Network::Regtest)
    .build();

  let ord_rpc_server =
    TestServer::spawn_with_server_args(&bitcoin_rpc_server, &["--regtest", "--index-runes"], &[]);

  create_wallet(&bitcoin_rpc_server, &ord_rpc_server);

  let a = etch(&bitcoin_rpc_server, &ord_rpc_server, Rune(RUNE));
  let b = etch(&bitcoin_rpc_server, &ord_rpc_server, Rune(RUNE + 1));

  pretty_assert_eq!(
    CommandBuilder::new("--index-runes --regtest runes")
      .bitcoin_rpc_server(&bitcoin_rpc_server)
      .run_and_deserialize_output::<Output>(),
    Output {
      runes: vec![
        (
          Rune(RUNE),
          RuneInfo {
            block: 8,
            burned: 0,
            divisibility: 0,
            etching: a.inscribe.reveal,
            id: RuneId { block: 8, tx: 1 },
            terms: None,
            mints: 0,
            number: 0,
            premine: 1000,
            rune: SpacedRune {
              rune: Rune(RUNE),
              spacers: 0
            },
            supply: 1000,
            symbol: Some('¢'),
            timestamp: ord::timestamp(8),
            tx: 1,
          }
        ),
        (
          Rune(RUNE + 1),
          RuneInfo {
            block: 16,
            burned: 0,
            divisibility: 0,
            etching: b.inscribe.reveal,
            id: RuneId { block: 16, tx: 1 },
            terms: None,
            mints: 0,
            number: 1,
            premine: 1000,
            rune: SpacedRune {
              rune: Rune(RUNE + 1),
              spacers: 0
            },
            supply: 1000,
            symbol: Some('¢'),
            timestamp: ord::timestamp(16),
            tx: 1,
          }
        )
      ]
      .into_iter()
      .collect(),
    }
  );
}
