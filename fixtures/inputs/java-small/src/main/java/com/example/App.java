package com.example;

import com.example.Helper;

/**
 * Application entry point.
 */
public class App {

    public static void main(String[] args) {
        Helper helper = new Helper();
        String result = helper.greet("world");
        System.out.println(result);
    }

    public String run(String name) {
        Helper helper = new Helper();
        return helper.greet(name);
    }
}
