*To test this runbook, give it to an engineer who was never on call for this service. Ask them to respond to an incident. When they stop, the runbook needs work.*

## Quick reference

*A person reads this first during an incident. Make it possible to read it in 30 seconds. Write facts only, and keep them current: an old fact is worse than no fact.*

| | |
|---|---|
| On call | [paging tool and service] |
| Chat | [alerts channel], [help channel] |
| Incident channel | [how the team opens it] |
| Production URL |  |
| Logs | [log tool and dashboard] |
| Dashboards |  |
| Deployment | [pipeline or deploy procedure] |
| Repository |  |
| Escalation | See Escalation path. |

## Service overview

*Write two to four sentences: what the service does, who uses it, and what breaks when it fails. Say how large the damage is, not how the service is built.*

### Criticality

*Give the lowest severity of a failure, and why. Say if a failure wakes a person at night, or if it can wait for working hours.*

| Lowest severity | Reason |
|---|---|
|  |  |

### Dependencies

*List each system that the service uses: databases, APIs, queues. Say what breaks in this service when each one fails.*

| Dependency | Type | Effect when it fails |
|---|---|---|
|  |  |  |

## Health checks

*Say what a healthy service shows. A person must be able to confirm the health in two minutes. Give exact commands, URLs or dashboard panels.*

### Signs of a healthy service

- 

### How to check health

```
[command, URL or steps]
```

## Failure modes

*Write one section for each failure that you know. Write it for a person who did not build the service. Each step is an action. When the answer is "it depends", split it into cases. A failure that caused an incident before goes here.*

### [Failure, as the person on call sees it]

**Symptoms:** what the alert says, what users report, what the logs show.

**Likely causes**

- 

**Diagnosis**

```
[commands or steps that find the cause]
```

**Resolution**

```
[steps for each cause]
```

**Escalate when:** [condition, for example "the service is not back 15 minutes after these steps"]

## Procedures

*Link the procedures that the person on call can need fast. The steps stay in the procedure.*

| Procedure | When to use it | Link |
|---|---|---|
|  |  |  |

## Escalation path

*Name people, not only roles. Keep it current: an old escalation path fails at the worst time.*

| Severity | First contact | How to reach them | Escalation |
|---|---|---|---|
|  |  |  |  |

## Known issues

*Write what the design does not show. For example: what failed before, what behaves strangely under load, what fails more often than it should.*

- 

## Maintenance

*Review this runbook after each incident that used it. Also review it each quarter, and after each change of the architecture or the dependencies.*

## Exit criteria

*Publish the runbook only when each item is true.*

- [ ] An engineer who is new to the service can follow it.
- [ ] The quick reference is current.
- [ ] The runbook was reviewed after the last incident.
