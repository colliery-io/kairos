Feature: Start from the base index

  # COLLIERY-T-1854. A Kairos server runs in the test, over a scratch
  # database of the dev stack. Its repository `payments-api` is a small
  # Python git repository, and the checkout is a clone of it. The `kairos`
  # binary of the test has no summarizer, and it connects with a
  # service-account key (`KAIROS_URL`, `KAIROS_KEY`).

  Scenario: A new checkout downloads, then updates
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 3 changed files and no local index
    When I run "kairos index update"
    Then the CLI downloads the index of A
    And it summarizes only the changed symbols

  Scenario: A far branch is told to rebase
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 250 changed files
    When I run "kairos index update"
    Then the CLI builds nothing
    And it tells me to rebase on main, or to run "kairos index --full"
    And it gives the time of a full build

  # KAIROS-T-0654: with --json, the update prints one JSON object. A
  # refusal is the error envelope on stderr, with a code and its counts.
  Scenario: kairos index update --json says where the update started
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 3 changed files and no local index
    When I run "kairos index update --json"
    Then the output is one JSON object: the update started from the index of A, with 3 changed files

  Scenario: A refusal with --json is the error envelope on stderr
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 250 changed files
    When I run "kairos index update --json"
    Then stdout is empty, and stderr has the error TOO_FAR: 250 changed files, the limit is 200

  # KAIROS-T-0646: a worktree has the [[project]] of its clone, because
  # the entry matches the origin remote (KAIROS-T-0742: the CLI does not
  # read .claude/kairos.local.md).
  Scenario: kairos index update in a worktree has the project of its clone
    Given Kairos has an index of commit A of main
    And a worktree of a wired clone, on a branch from A with 3 changed files
    When I run "kairos index update" in the worktree, with no KAIROS_URL and no --repo
    Then the CLI downloads the index of A

  # KAIROS-T-0646: kairos repos local matches the origin remote of each
  # clone with the repositories of Kairos. It counts from the local refs.
  Scenario: kairos repos local gives the repository of each clone of a board folder
    Given Kairos has an index of commit A of main
    And a board folder with a clone 1 commit behind, a worktree of it and a clone of another repository
    When I run "kairos repos local --json" on the board folder, with no KAIROS_URL
    Then the list has payments-api: repository payments-api, clone, 1 behind origin/main
    And the list has wt-payments: repository payments-api, worktree of payments-api, 0 behind origin/main
    And the list has other with no repository and no count
    And the list has no other entry

  # KAIROS-T-0516: a clone that is only behind gets the index of the tip
  # of origin/main, when its diff is smaller than the diff of the index
  # below the checkout.
  Scenario: A checkout behind origin/main starts from the index of the tip
    Given Kairos has an index of commit A of main
    And main then has commit C with 250 new files, then commit B with 1 change
    And Kairos has an index of commit B
    And a checkout of main at C, behind origin/main at B
    When I run "kairos index update"
    Then the CLI downloads the index of B, 1 file changed since it
    And the index of the checkout has the 250 new files

  # KAIROS-T-0515: an update that runs no model has no default limit of
  # changed files. It parses the changed files and links the pool.
  Scenario: A link-only update of a branch with 500 changed files starts from the base
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 500 changed files and no local index
    When I run "kairos index update --link-only"
    Then the CLI downloads the index of A, with 500 changed files
    And the index of the checkout has the 500 new files

  Scenario: fetch_index opens a branch with 500 changed files
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 500 changed files and no local index
    And a kairos-code server started in a folder that is not a checkout
    When the agent calls fetch_index with the path of the checkout
    Then the server says that the index of A is open, with 500 changed files
    And the index of the checkout has the 500 new files

  # KAIROS-T-0517: the agent gives a limit. The refusal names it, and says
  # that no checkout is open.
  Scenario: fetch_index with max_changed refuses a far branch
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 250 changed files and no local index
    And a kairos-code server started in a folder that is not a checkout
    When the agent calls fetch_index with the path of the checkout and max_changed 10
    Then the server refuses: 250 files changed, the limit is 10, give max_changed 250
    And the server says that no checkout is open

  Scenario: A machine with no model shows the summaries of the base index
    Given Kairos has a summarized index of commit A of main
    And a kairos binary with no summarizer
    When I run "kairos index update" on a checkout of A
    Then each symbol of A has its summary from the base index
    And no model ran

  Scenario: With no Kairos, the local index is updated
    Given a checkout with a local index and no connection to Kairos
    When I change a file and run "kairos index update"
    Then the local index describes the changed file
    And the CLI says that it could not reach Kairos

  # KAIROS-T-0348: a session that starts outside a checkout (a board folder
  # with the clones below it) has no index. The agent opens the checkout
  # with the tool `fetch_index`: the server takes the nearest index from
  # Kairos and answers the other tools from it. No hook, no CLI command.
  Scenario: An agent outside a checkout opens the index of a clone with fetch_index
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 3 changed files and no local index
    And a kairos-code server started in a folder that is not a checkout
    When the agent calls code_search
    Then the server refuses: no checkout is open, call fetch_index
    When the agent calls fetch_index with the path of the checkout
    Then the server says that the index of A is open, with 3 changed files
    And module_map answers for the checkout
    And the checkout has the index of A, updated for its changed files

  # KAIROS-T-0343: on a repository that opted in to the hosted provider of
  # its organization, the CLI makes no summary and never sees the key of the
  # organization. It links the pool of the base index, and the structure of
  # the checkout is still built for the changed files.
  Scenario: A hosted repository makes no summaries in the checkout
    Given Kairos has a summarized index of commit A of main
    And the repository is on the hosted provider of the organization
    And a checkout of a branch from A with 3 changed files and no local index
    When I run "kairos index update" with KAIROS_INDEX_SUMMARIZE=1
    Then the CLI downloads the index of A
    And the CLI says that this repository makes its summaries on Kairos
    And the symbols of A keep their summaries, and the changed symbols have none

  # KAIROS-T-0355: the builder of Kairos made more summaries of the commit
  # of the local index (a push run). The local index is as near to the tree
  # as the base, so it keeps its structure, takes the pool of the base and
  # links the new summaries.
  Scenario: kairos index update takes the new summaries of the base index of the same commit
    Given Kairos has a partly summarized index of commit A of main
    And a checkout of A whose local index links the summaries of that index
    And Kairos then gets a summary of each symbol of A
    When I run "kairos index update --link-only"
    Then the update says that it took the summaries from Kairos
    And each symbol of A has its summary from the base index

  Scenario: fetch_index takes the new summaries of the base index of the same commit
    Given Kairos has a partly summarized index of commit A of main
    And a checkout of A whose local index links the summaries of that index
    And Kairos then gets a summary of each symbol of A
    And a kairos-code server started in a folder that is not a checkout
    When the agent calls fetch_index with the path of the checkout
    Then the server says that it took the summaries from Kairos
    And each symbol of A has its summary from the base index

  # KAIROS-T-0592: a deployment with no builder of the code index gets its
  # base indexes from `kairos index upload`, for example in a CI job.
  Scenario: kairos index upload sends the index of the checkout for its HEAD commit
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 3 changed files and no local index
    When I run "kairos index update"
    And I run "kairos index upload"
    Then Kairos has an index of the HEAD commit of the checkout, for the ref feature

  Scenario: kairos index upload refuses a tree with changes that are not committed
    Given Kairos has an index of commit A of main
    And a checkout of a branch from A with 3 changed files and no local index
    When I run "kairos index update"
    And a file of the checkout changes, with no commit
    And I run "kairos index upload"
    Then the CLI refuses the upload, because pkg/stats.py is not committed
