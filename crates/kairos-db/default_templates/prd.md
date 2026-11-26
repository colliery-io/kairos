## Context

*Optional when a Design Doc exists: write its code. Else, say the problem and the capability areas in a few sentences.*

## Requirements

*A requirement says a behavior that the system guarantees, not how the system does it. A test must be able to check it. Use "must", not "should". Number each one: REQ-area.number.*

### 1. Capability area

*Use the capability areas of the Design Doc. Say what this area owns.*

**REQ-1.1: Name.** What the system must do, in one to three sentences. Give the reason when it is not clear.

### Cross-cutting requirements

*Write the requirements for all areas: onboarding, ownership, change management.*

**REQ-X.1: Name.** 

## Non-functional requirements

*Each one has a target that you can measure, or a condition that you can test. Write "a lookup adds less than 10 ms", not "fast".*

### Performance

**NFR-1.1: Name.** 

### Availability and reliability

*Say what continues to work when a part fails, and what the system can lose.*

**NFR-2.1: Name.** 

### Security

**NFR-3.1: Name.** 

### Service levels

*Give the service level objective of each main workflow. It must agree with the service levels of the systems that this system uses. It must also agree with the systems that use it. When you do not know them, write that as a risk with an owner.*

**NFR-4.1: Name.** 

### Escalation

*For each failure that you know or expect, say who gets the alert, and by which tool. Say how fast, and who owns the response. Cover at least: a failed dependency, late or old data, and the service down.*

**NFR-5.1: Escalation for [failure].** When [condition], [team or role] gets an alert by [paging tool] in [time]. [Team] owns the response in [time].

## Out of scope

*List what this PRD does not cover, so that nobody expects it.*

## Open questions

*List what you do not know yet, and who owns each answer.*

## Exit criteria

*Publish the PRD only when each item is true.*

- [ ] Each requirement has a number and a test can check it.
- [ ] Each non-functional requirement has a target or a condition.
- [ ] The people who own the product reviewed and approved it.
