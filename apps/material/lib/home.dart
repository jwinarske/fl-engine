// A Scaffold with a filled, rounded Material card, centered: no text, no
// elevation (shadows are not mapped yet).
import 'package:flutter/material.dart';

class Home extends StatelessWidget {
  const Home({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFFFFF8E1),
      body: Center(
        child: SizedBox(
          width: 200,
          height: 120,
          child: Material(
            color: const Color(0xFF6750A4),
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(24)),
            child: const SizedBox.expand(),
          ),
        ),
      ),
    );
  }
}
