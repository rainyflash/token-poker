export async function dispatchHostCommand(runtime, command, requestId) {
  if (command.type === "set_volunteer_consent") {
    await runtime.setVolunteerConsent(command.enabled);
    return "accepted";
  }
  if (command.type === "ensure_identity" || command.type === "restore_identity") {
    await runtime.ensureIdentity(command, requestId);
    return "confirmed";
  }
  if (command.type === "leave_table") {
    await runtime.leaveTable(command, requestId);
    return "confirmed";
  }
  await runtime.send(command);
  return "accepted";
}
