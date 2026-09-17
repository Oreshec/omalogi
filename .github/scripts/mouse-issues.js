// Keeps one issue per mouse model. A new Mouse report for a model that already has an open
// issue is pointed there and closed; otherwise it joins the tracking issue, and a
// verification is assigned to its author as the claim on that model. On a model's issue,
// a `/claim` comment claims it and `/unclaim` gives it back.

const TRACKING_ISSUE = 15
const LABEL = "verify-a-mouse"
const VERIFY_GOAL = /^verify it/i
const REQUEST_GOAL = /^ask for a model/i

// The answers of an issue form, keyed by question. GitHub renders each as a "### " heading
// followed by the answer, or "_No response_" when it was left empty.
function parseForm(body) {
  const fields = {}
  for (const section of String(body || "").split(/^### /m).slice(1)) {
    const newline = section.indexOf("\n")
    if (newline < 0) continue
    const value = section.slice(newline + 1).trim()
    fields[section.slice(0, newline).trim()] = value === "_No response_" ? "" : value
  }
  return fields
}

// A product id as four lowercase hex digits: "046d:c08b", "C08B" and "0x4074" all name one.
function productId(text) {
  const words = String(text || "").toLowerCase().replace(/046d:|0x/g, " ").match(/\b[0-9a-f]{4}\b/g) || []
  return words.find((word) => word !== "046d") || null
}

// A model name safe to repeat in a comment: no markup, mentions or issue references.
function plainName(text) {
  return String(text || "").replace(/[^\w .+-]/g, "").replace(/\s+/g, " ").trim().slice(0, 60)
}

// The product id a Mouse report, or an older device report, names.
function reportedId(body) {
  const fields = parseForm(body)
  return productId(fields["USB id or wireless id"] || fields["USB id"])
}

// The open issue already tracking this product id, other than the new one.
function trackingIssue(issues, id, number) {
  return issues.find((issue) =>
    issue.number !== number &&
    issue.number !== TRACKING_ISSUE &&
    !issue.pull_request &&
    reportedId(issue.body) === id) || null
}

// The command a comment starts with: "claim", "unclaim" or null.
function command(body) {
  const match = String(body || "").trim().match(/^\/(claim|unclaim)\b/i)
  return match ? match[1].toLowerCase() : null
}

async function claim({ github, context }) {
  const { owner, repo } = context.repo
  const { issue, comment } = context.payload
  const action = command(comment.body)
  if (!action || issue.pull_request || issue.state !== "open" || issue.number === TRACKING_ISSUE) return
  if (!issue.labels.some((label) => label.name === LABEL)) return

  const login = comment.user.login
  const assignees = issue.assignees.map((user) => user.login)
  const react = (content) => github.rest.reactions.createForIssueComment({
    owner, repo, comment_id: comment.id, content
  })

  if (action === "claim") {
    if (assignees.includes(login)) return react("+1")
    if (assignees.length > 0) {
      await github.rest.issues.createComment({
        owner, repo, issue_number: issue.number,
        body: `This model is already claimed by @${assignees[0]}. A claim with no update for 30 days is released; if that is the case here, say so in this issue.`
      })
      return
    }
    await github.rest.issues.addAssignees({ owner, repo, issue_number: issue.number, assignees: [login] })
    return react("rocket")
  }

  if (!assignees.includes(login)) return
  await github.rest.issues.removeAssignees({ owner, repo, issue_number: issue.number, assignees: [login] })
  return react("+1")
}

async function run({ github, context }) {
  const { owner, repo } = context.repo
  const issue = context.payload.issue
  if (issue.number === TRACKING_ISSUE) return

  const fields = parseForm(issue.body)
  const id = productId(fields["USB id or wireless id"])
  if (!id) return
  const goal = fields["What would you like to do?"] || ""
  const model = plainName(fields["Mouse model"]) || "mouse"

  const open = await github.paginate(github.rest.issues.listForRepo, {
    owner, repo, state: "open", labels: LABEL, per_page: 100
  })
  const existing = trackingIssue(open, id, issue.number)
  if (existing) {
    const next = VERIFY_GOAL.test(goal)
      ? "Comment `/claim` there to take the verification, unless someone already has it."
      : "Please add your report there."
    await github.rest.issues.createComment({
      owner, repo, issue_number: issue.number,
      body: `Thanks! The ${model} (${id}) is already tracked in #${existing.number}. ${next} One issue per model keeps everything about it in one place, so this one is closed as a duplicate.`
    })
    await github.rest.issues.update({
      owner, repo, issue_number: issue.number, state: "closed", state_reason: "duplicate"
    })
    return
  }

  if (!REQUEST_GOAL.test(goal)) {
    await github.request("POST /repos/{owner}/{repo}/issues/{issue_number}/sub_issues", {
      owner, repo, issue_number: TRACKING_ISSUE, sub_issue_id: issue.id
    })
  }
  if (VERIFY_GOAL.test(goal)) {
    await github.rest.issues.addAssignees({
      owner, repo, issue_number: issue.number, assignees: [issue.user.login]
    })
    await github.rest.issues.createComment({
      owner, repo, issue_number: issue.number,
      body: [
        `Thanks, the ${model} (${id}) is yours to verify and is assigned to you.`,
        "",
        "Open your coding agent in a clone of this repository and give it `docs/agents/verify-my-mouse.md`. It walks you through the backup, the self-test and the pull request. Mention this issue in the pull request with `Closes #" + issue.number + "`.",
        "",
        "If the self-test fails, post its summary here; a failure on real hardware is as useful as a pass. If you cannot finish, comment `/unclaim`. A claim with no update for 30 days is released so someone else can pick the model up."
      ].join("\n")
    })
  }
}

module.exports = { parseForm, productId, plainName, reportedId, trackingIssue, command, claim, run }
