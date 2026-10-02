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
