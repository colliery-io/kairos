Feature: The CLI and the code tools

  # COLLIERY-T-1852. Each scenario uses a copy of the polyglot fixture of
  # kairos-index, with a git repository in it. An agent is this test: it
  # starts `kairos index mcp` and talks MCP to it over stdio.

  Scenario: Build, then read the status
    Given the polyglot fixture with no index
    When I run "kairos index build" and then "kairos index status"
    Then the status shows the count of files, symbols, edges and summaries
    And the index is in .kairos/index.db and git ignores it

  Scenario: An agent finds a symbol by what it does
    Given an index of the polyglot fixture
    When an agent calls code_search with a description of a Rust function
    Then that function is in the first 3 results

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

  Scenario: module_map replaces the Metis index
    Given an index of the polyglot fixture
    When an agent calls module_map
    Then the result has each module with its summary and its files

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
