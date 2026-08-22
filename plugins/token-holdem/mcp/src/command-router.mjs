export async function dispatchHostCommand(runtime, command) {
  if (command.type === "set_volunteer_consent") {
    await runtime.setVolunteerConsent(command.enabled);
    return;
  }
  await runtime.send(command);
}
