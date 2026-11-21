/// Records that the generated registrant bound it, as a federated plugin's
/// `registerWith` sets its platform interface's instance.
class DartOnlyPlugin {
  static bool registered = false;

  static void registerWith() {
    registered = true;
  }
}
