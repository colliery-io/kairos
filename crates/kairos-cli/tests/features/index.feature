Feature: The CLI and the code tools

  # COLLIERY-T-1852. Each scenario uses a copy of the polyglot fixture of
  # kairos-index, with a git repository in it. An agent is this test: it
  # starts `kairos index mcp` and talks MCP to it over stdio.

  Scenario: Build, then read the status
    Given the polyglot fixture with no index
    When I run "kairos index build" and then "kairos index status"
    Then the status shows the count of files, symbols, edges and summaries
    And the index is in .kairos/index.db and git ignores it

  # KAIROS-T-0654: with --json, each command prints one JSON object.
  Scenario: Build and status with --json give one JSON object each
    Given the polyglot fixture with no index
    When I run "kairos index build --json" and then "kairos index status --json"
    Then each output is one JSON object with the counts of the index

  Scenario: duplicates and references with --json give the list envelope
    Given an index of the polyglot fixture
    When I run "kairos index duplicates --json" and then "kairos index references Queue --file src/queue.rs --json"
    Then each output is the list envelope, with the rows of the text

  # The deterministic vectors of the tests come from a hash of the text, so
  # their rank is noise. These 2 scenarios check the text rank
  # (KAIROS-T-0423), so their index has no vectors.
  Scenario: An agent finds a symbol by what it does
    Given an index of the polyglot fixture whose summaries have no vectors
    When an agent calls code_search with a description of a Rust function
    Then that function is in the first 3 results

  # KAIROS-T-0421: test code has a summary, so the search finds a test by
  # what it checks. The result marks it as test code. The query says "test":
  # other queries rank test code below the other code (KAIROS-T-0422).
  Scenario: An agent finds a test by what it checks
    Given an index of the polyglot fixture whose summaries have no vectors
    When an agent calls code_search with what a test checks
    Then the first result is that test, marked as test code, with its summary

  # KAIROS-T-0422: a helper in a Rust tests/ folder is test harness code.
  Scenario: A helper in a Rust tests folder is test code
    Given an index of the polyglot fixture whose summaries have no vectors
    When an agent calls symbol, code_search and module_map for "helper" in "tests/common/mod.rs"
    Then each of the 3 results marks tests/common/mod.rs and its helper as test code

  Scenario: callers shows only certain edges by default
    Given an index where a Python function has 1 certain caller and 1 possible caller
    When an agent calls callers for that function
    Then the result has the certain caller only
    When the agent calls callers with possible edges on
    Then the result has the 2 callers, and it marks the possible one

  Scenario: path finds how one symbol reaches another
    Given an index of the polyglot fixture
    When an agent calls path from the Rust entry point to a Rust function 3 calls deep
    Then the result is the 3 calls in order

  Scenario: An unknown argument is refused
    When an agent calls symbol with an argument that the tool does not have
    Then the call is refused, and the error names the argument

  # KAIROS-T-0417: a Rust struct and its impl block have the same name.
  Scenario: An agent gets a type, not its impl block
    Given an index of the polyglot fixture
    When an agent calls symbol and callers for "Checksum" in "src/checksum.rs"
    Then the 2 results are about the struct
    When an agent calls symbol for "Checksum" with no file
    Then the error lists the 2 structs and tells the agent to give the file
    And each candidate in the list, given back as the symbol, gives that symbol

  # KAIROS-T-0420: the source of a symbol.
  Scenario: An agent gets the source of a symbol
    Given an index of the polyglot fixture
    When an agent calls symbol for "Checksum" in "src/checksum.rs" with source
    Then the result has the source of the struct, from the checkout
    When the file "src/checksum.rs" changes and the agent calls symbol with source again
    Then the result says that the source can be different from the symbol

  # KAIROS-T-0419: callers missed the uses of a type in a signature and the
  # uses of its fields.
  Scenario: references gives each use of a symbol, not only its calls
    Given an index of the polyglot fixture
    When an agent calls references for "Queue" in "src/queue.rs"
    Then the result has the uses of the type by file, each with its line, its kind and the symbol that holds it
    And "kairos index references Queue --file src/queue.rs" gives the same text
    When an agent calls references for "Report" in "python/polyglot/report.py"
    Then the result has the 2 calls, and it says that it has the calls only for python code

  # KAIROS-T-0703: references takes an ADR short code, and gives each place
  # that cites the ADR.
  Scenario: references gives each place that cites an ADR
    Given an index of the polyglot fixture, where 2 files cite "KAIROS-A-0065"
    When an agent calls references for "KAIROS-A-0065"
    Then the result has the 3 places that cite "KAIROS-A-0065" by file, each with its line, its context and its symbol
    And "kairos index references KAIROS-A-0065" gives the same text
    When an agent calls references for "KAIROS-A-0065" in "src/queue.rs"
    Then the call is refused, because an ADR short code takes no file
    When an agent calls references for "KAIROS-A-0999"
    Then the result says that no place cites "KAIROS-A-0999"

  Scenario: An unknown argument of references is refused
    When an agent calls references with an argument that the tool does not have
    Then the call is refused, and the error names the argument

  Scenario: module_map replaces the Metis index
    Given an index of the polyglot fixture
    When an agent calls module_map
    Then the result has each module with its summary and its files
    And the Terraform module shows its resources and the root shows its module calls

  # COLLIERY-T-1857: repeated code, over MCP and from the CLI.
  Scenario: An agent finds repeated code
    Given an index of the polyglot fixture
    When an agent calls duplicates
    Then the result has the exact copy of checksum, with the files, the lines and a score
    And "kairos index duplicates" gives the same groups

  Scenario: An unknown argument of duplicates is refused
    When an agent calls duplicates with an argument that the tool does not have
    Then the call is refused, and the error names the argument

  # COLLIERY-T-2531: path gives up to 3 chains, shortest first.
  Scenario: path gives more than one chain
    Given the polyglot fixture, where main reaches target by 2 different routes
    When an agent calls path from main to target
    Then the result has the 2 routes, shortest first

  # #272: a session in a folder above several checkouts opens more than one
  # index. code_search searches each of them; the other tools read the last
  # one opened, or the one that `checkout` names.
  Scenario: An agent searches 2 open checkouts and reads one of them by name
    Given a folder with 2 indexed checkouts, service and engine, where only engine has engine_only
    When an agent started in that folder calls fetch_index for service, then for engine
    Then the second fetch_index says that 2 checkouts are open: service, engine
    When the agent calls code_search for "the mean of the values"
    Then the results come from both checkouts, each marked with its name
    When the agent calls symbol for "engine_only" with no checkout
    Then the result is the function of engine
    When the agent calls symbol for "engine_only" in the checkout "service"
    Then the server says that no symbol has that name
    When the agent calls symbol for "engine_only" in the checkout "nowhere"
    Then the call is refused, and the error names the 2 open checkouts
