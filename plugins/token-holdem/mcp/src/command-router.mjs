export async function dispatchHostCommand(runtime, command, requestId) {
  if (command.type === "set_volunteer_consent") {
    await runtime.setVolunteerConsent(command.enabled);
    return { status: "accepted" };
  }
  if (["ensure_identity", "create_identity", "restore_identity", "restore_remote_identity"].includes(command.type)) {
    const identity = await runtime.ensureIdentity(command, requestId);
    return { status: "confirmed", identity_confirmation: identity };
  }
  if (command.type === "leave_table") {
    await runtime.leaveTable(command, requestId);
    return { status: "confirmed" };
  }
  if (command.type === "submit_action") {
    await runtime.submitAction(command, requestId);
    return { status: "confirmed" };
  }
  await runtime.send(command);
  return { status: "accepted" };
}
